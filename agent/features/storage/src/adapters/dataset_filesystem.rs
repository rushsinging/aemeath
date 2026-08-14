//! #983 `FileSystemDatasetAdapter`：受 cap-std 约束目录下的完整代数据集事务
//! 与崩溃恢复协议。
//!
//! 每个 `DatasetKey` 独占一个 OS 排他锁，所有入口都先取锁再跑一次恢复。提交严格
//! 遵循「写 stage → 保留 previous → 完整 fsync → 写 Prepared journal（逻辑提交点）
//! → 逐 member 发布/删除 omitted → validate 全代 → Committed marker → 提升 previous
//! → cleanup」。Prepared 落盘即视为已提交：恢复只前滚，绝不回滚。
//!
//! 协议独立于 blob，不复用 `AtomicBlobPort`。

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use async_trait::async_trait;
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use fs2::FileExt;
use uuid::Uuid;

use super::dataset_protocol as proto;
use super::dataset_protocol::{
    DatasetJournal, DatasetManifestMemberRecord, DatasetManifestRecord, JournalKind, JournalMember,
    JournalPhase, BLOBS_DIR, CORRUPTION_MARKER, JOURNAL_FILE, LOCK_FILE, MANIFEST_FILE,
    MEMBERS_DIR, PREVIOUS_DIR, PREVIOUS_NEXT_DIR, PRIMARY_DIR,
};
use crate::domain::revision_member_digest;
use crate::{
    AtomicDatasetPort, CommitWarning, CorruptTransactionError, CorruptionReason, DatasetChangeSet,
    DatasetCommitReceipt, DatasetCommitVisibility, DatasetKey, DatasetManifest, DatasetMember,
    DatasetMemberChange, DatasetMemberReference, DatasetRead, DatasetReadOutcome, DatasetRevision,
    DeleteOptions, DeleteOutcome, Generation, QuarantineDisposition, QuarantineOutcome,
    QuarantineReason, QuarantineReceipt, SafePathSegment, StorageError, StorageErrorKind,
    TransactionScope, WriteOptions,
};

// ---- 私有、仅供 crash 测试驱动的故障注入接缝（不对外导出） ----

#[cfg(any(test, feature = "test-fault-injection"))]
const FAULT_POINT_ENV: &str = "AEMEATH_STORAGE_DATASET_FAULT_POINT";
#[cfg(any(test, feature = "test-fault-injection"))]
const FAULT_ABORT_ENV: &str = "AEMEATH_STORAGE_DATASET_FAULT_ABORT";

/// 受测的 post-Prepared 故障点。
enum FaultPoint {
    AfterPrepared,
    AfterMemberPublish(#[allow(dead_code)] String),
    PromoteAfterPrepared,
    PromoteAfterPrimaryToSwap,
    PromoteAfterPreviousToPrimary,
}

#[cfg(any(test, feature = "test-fault-injection"))]
fn fault_requested(point: &FaultPoint) -> bool {
    let Some(requested) = std::env::var_os(FAULT_POINT_ENV) else {
        return false;
    };
    let requested = requested.to_string_lossy();
    match point {
        FaultPoint::AfterPrepared => requested == "after_prepared",
        FaultPoint::AfterMemberPublish(name) => requested == format!("after_member_publish:{name}"),
        FaultPoint::PromoteAfterPrepared => requested == "promote_after_prepared",
        FaultPoint::PromoteAfterPrimaryToSwap => requested == "promote_after_primary_to_swap",
        FaultPoint::PromoteAfterPreviousToPrimary => {
            requested == "promote_after_previous_to_primary"
        }
    }
}

/// 在 post-Prepared 故障点触发注入。命中且设置了 `FAULT_ABORT` → 直接 abort；
/// 否则返回一个普通 I/O 错误，由提交路径转化为 committed `RecoveryPending` 收据。
#[cfg(any(test, feature = "test-fault-injection"))]
fn inject_post_prepared_fault(point: &FaultPoint) -> Result<(), StorageError> {
    if !fault_requested(point) {
        return Ok(());
    }
    if std::env::var_os(FAULT_ABORT_ENV).is_some() {
        std::process::abort();
    }
    Err(StorageError::new(
        StorageErrorKind::Io,
        "注入的数据集事务故障（post-Prepared）",
    ))
}

#[cfg(not(any(test, feature = "test-fault-injection")))]
fn inject_post_prepared_fault(_point: &FaultPoint) -> Result<(), StorageError> {
    Ok(())
}

/// 受约束目录内的多成员数据集事务 adapter。
pub struct FileSystemDatasetAdapter {
    root: Dir,
}

impl FileSystemDatasetAdapter {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        log::debug!(target: crate::LOG_TARGET, "dataset_adapter init enter");
        let result = (|| {
            std::fs::create_dir_all(root.as_ref()).map_err(proto::map_io)?;
            let root =
                Dir::open_ambient_dir(root.as_ref(), ambient_authority()).map_err(proto::map_io)?;
            Ok::<Self, StorageError>(Self { root })
        })();
        match &result {
            Ok(_) => log::info!(target: crate::LOG_TARGET, "dataset_adapter init ok"),
            Err(_) => log::error!(target: crate::LOG_TARGET, "dataset_adapter init failed"),
        }
        result
    }

    fn dataset_rel(key: &DatasetKey) -> PathBuf {
        key.segments()
            .iter()
            .fold(PathBuf::from(key.namespace().as_str()), |path, segment| {
                path.join(segment.as_str())
            })
    }

    fn open_dataset_dir(&self, key: &DatasetKey) -> Result<Dir, StorageError> {
        let rel = Self::dataset_rel(key);
        self.root.create_dir_all(&rel).map_err(proto::map_io)?;
        self.root.open_dir(&rel).map_err(proto::map_io)
    }

    fn open_existing_dataset_dir(&self, key: &DatasetKey) -> Result<Option<Dir>, StorageError> {
        let rel = Self::dataset_rel(key);
        match self.root.symlink_metadata(&rel) {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "数据集路径是符号链接",
            )),
            Ok(metadata) if !metadata.file_type().is_dir() => Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "数据集路径不是目录",
            )),
            Ok(_) => self.root.open_dir(&rel).map(Some).map_err(proto::map_io),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(proto::map_io(error)),
        }
    }

    fn collect_dataset_keys(
        &self,
        directory: &Dir,
        namespace: crate::domain::StorageNamespace,
        segments: &mut Vec<SafePathSegment>,
        result: &mut Vec<DatasetKey>,
    ) -> Result<(), StorageError> {
        for entry in directory.entries().map_err(proto::map_io)? {
            let entry = entry.map_err(proto::map_io)?;
            let name = entry.file_name();
            let metadata = entry.metadata().map_err(proto::map_io)?;
            if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                continue;
            }
            let name = match SafePathSegment::from_str(&name.to_string_lossy()) {
                Ok(name) => name,
                Err(_) => continue,
            };
            let child = directory
                .open_dir(Path::new(name.as_str()))
                .map_err(proto::map_io)?;
            segments.push(name);
            if proto::exists(&child, Path::new(PRIMARY_DIR).join(MANIFEST_FILE).as_path())? {
                result.push(DatasetKey::new(namespace, segments.clone())?);
            }
            self.collect_dataset_keys(&child, namespace, segments, result)?;
            segments.pop();
        }
        Ok(())
    }

    fn list_datasets_sync(
        &self,
        namespace: crate::domain::StorageNamespace,
    ) -> Result<Vec<DatasetKey>, StorageError> {
        let namespace_path = Path::new(namespace.as_str());
        let namespace_dir = match self.root.open_dir(namespace_path) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(proto::map_io(error)),
        };
        let mut result = Vec::new();
        self.collect_dataset_keys(&namespace_dir, namespace, &mut Vec::new(), &mut result)?;
        result.sort_by(|left, right| left.segments().cmp(right.segments()));
        Ok(result)
    }

    fn delete_all_generations_sync(
        &self,
        key: &DatasetKey,
        options: DeleteOptions,
    ) -> Result<DeleteOutcome, StorageError> {
        let rel = Self::dataset_rel(key);
        let Some(dir) = self.open_existing_dataset_dir(key)? else {
            return Ok(DeleteOutcome::new(false, false, false));
        };
        let lock = self.lock(&dir)?;
        self.recover(&dir)?;
        let deleted_primary = proto::exists(&dir, Path::new(PRIMARY_DIR))?;
        let deleted_previous = proto::exists(&dir, Path::new(PREVIOUS_DIR))?;
        let deleted_quarantine = false;
        if options.include_quarantine() {
            drop(lock);
            self.root.remove_dir_all(&rel).map_err(proto::map_io)?;
        } else {
            for generation in [PRIMARY_DIR, PREVIOUS_DIR] {
                if proto::exists(&dir, Path::new(generation))? {
                    dir.remove_dir_all(generation).map_err(proto::map_io)?;
                }
            }
            proto::sync_dir(&dir)?;
            drop(lock);
        }
        Ok(DeleteOutcome::new(
            deleted_primary,
            deleted_previous,
            deleted_quarantine,
        ))
    }
    fn lock(&self, dir: &Dir) -> Result<std::fs::File, StorageError> {
        proto::reject_symlink(dir, Path::new(LOCK_FILE))?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        let lock = dir
            .open_with(LOCK_FILE, &options)
            .map_err(proto::map_io)?
            .into_std();
        #[cfg(feature = "test-fault-injection")]
        if let Some(marker) = std::env::var_os("AEMEATH_STORAGE_DATASET_LOCK_ATTEMPT") {
            std::fs::write(marker, b"attempt").map_err(proto::map_io)?;
        }
        lock.lock_exclusive().map_err(proto::map_lock_io)?;
        Ok(lock)
    }

    /// 取锁并跑一次恢复，返回数据集目录句柄与锁守卫。所有入口都经此路径：
    /// 锁内恢复保证并发读永不看到半代，且校验前滚合法性。
    fn locked(&self, key: &DatasetKey) -> Result<(Dir, std::fs::File), StorageError> {
        let dir = self.open_dataset_dir(key)?;
        let lock = self.lock(&dir)?;
        self.recover(&dir)?;
        self.collect_orphan_member_content_best_effort(&dir);
        Ok((dir, lock))
    }

    /// 回收不被 primary / previous 任一代 manifest 引用的成员内容文件（孤儿）。
    ///
    /// 每次增量修订都会让 envelope / state / metadata 类成员产生新内容摘要，被替换
    /// 的旧内容在 previous 代被下一次提交覆盖后即无任何引用。本清理在每个数据集
    /// 入口（recover 后）与每次提交收尾时执行，使 `members/` 稳态收敛为
    /// 「primary + previous 两代引用集」。
    ///
    /// 纯清理动作：被删除的文件不被任何存活代引用，中断后由下一次入口补跑，
    /// 绝不影响提交语义与崩溃恢复。legacy 布局的成员内容位于各代 blobs/ 目录，
    /// 不在共享 `members/` 内，天然不受影响。
    fn collect_orphan_member_content(&self, dir: &Dir) -> Result<usize, StorageError> {
        let mut referenced_digests = BTreeSet::new();
        for generation_dir in [PRIMARY_DIR, PREVIOUS_DIR] {
            if let Some(record) = proto::read_manifest(dir, Path::new(generation_dir))? {
                referenced_digests.extend(
                    record
                        .成员证据
                        .iter()
                        .filter(|member| !member.内容摘要.is_empty())
                        .map(|member| member.内容摘要.clone()),
                );
            }
        }
        let member_store = PathBuf::from(MEMBERS_DIR);
        if !proto::exists(dir, &member_store)? {
            return Ok(0);
        }
        let member_dir = dir.open_dir(&member_store).map_err(proto::map_io)?;
        let mut removed_count = 0usize;
        for entry in member_dir.entries().map_err(proto::map_io)? {
            let entry = entry.map_err(proto::map_io)?;
            let file_type = entry.file_type().map_err(proto::map_io)?;
            // 只回收普通文件；目录与符号链接不是内容寻址存储的合法形态，保持原位
            // 由隔离 / 人工排查处理，绝不递归删除。
            if !file_type.is_file() {
                continue;
            }
            let content_digest = entry.file_name().to_string_lossy().into_owned();
            if referenced_digests.contains(content_digest.as_str()) {
                continue;
            }
            member_dir
                .remove_file(&content_digest)
                .map_err(proto::map_io)?;
            removed_count += 1;
        }
        if removed_count > 0 {
            proto::sync_subdir(dir, &member_store)?;
        }
        Ok(removed_count)
    }

    /// best-effort 孤儿回收：失败仅告警，绝不阻断读写入口——回收延迟一个入口
    /// 只影响磁盘占用，不影响任何存活代的数据完整性。
    fn collect_orphan_member_content_best_effort(&self, dir: &Dir) {
        match self.collect_orphan_member_content(dir) {
            Ok(_) => {}
            Err(_) => {
                log::warn!(target: crate::LOG_TARGET, "dataset_orphan_collection_pending");
            }
        }
    }

    // ---- 崩溃恢复 ----

    /// 锁内恢复：无 journal → 清理 pre-Prepared 残留；journal 无法解码或结构非法
    /// → InvalidJournal 隔离；否则按 journal 前滚（提交前滚成员发布；提升前滚交换）。
    fn recover(&self, dir: &Dir) -> Result<(), StorageError> {
        // 持久损坏标记优先于一切：无法隔离的矛盾 primary 会落盘此标记，恢复入口据此
        // 持续 fail-closed，绝不再触碰 journal / primary（journal 作为屏障原位保留）。
        if proto::exists(dir, Path::new(CORRUPTION_MARKER))? {
            return Err(corruption_marker_error());
        }
        // 隔离名下的 journal 也是持久屏障。它可能来自 quarantine 在 marker 落盘后、
        // primary 搬运前的崩溃；绝不能把这种状态当作「无 journal」继续打开 primary。
        if proto::has_corrupt_journal(dir)? {
            return Err(corrupt_journal_barrier_error());
        }
        let Some(journal_result) = proto::read_journal_raw(dir)? else {
            return self.cleanup_orphans(dir);
        };
        let journal = match journal_result {
            Ok(journal) => journal,
            Err(()) => {
                return Err(self.quarantine_corrupt(dir, CorruptionReason::InvalidJournal, false));
            }
        };
        // 在用 journal 的任何字段拼接磁盘路径之前，先做纯结构语义校验。
        // 结构非法（不安全成员名、坏摘要/修订号、非严格升序/重复）一律 InvalidJournal
        // 隔离，且绝不移动健康的 primary 代。
        if proto::validate_journal_structure(&journal).is_err() {
            return Err(self.quarantine_corrupt(dir, CorruptionReason::InvalidJournal, false));
        }
        match journal.操作 {
            JournalKind::Commit => self.roll_forward_commit(dir, &journal),
            JournalKind::PromotePrevious => self.roll_forward_promote(dir, &journal),
        }
    }

    /// 前滚一个已达逻辑提交点的完整替换提交。
    ///
    /// 先整代校验（不改任何状态），任一成员的 stage/已发布字节与 journal 记录的新代
    /// 摘要矛盾即 fail-closed 隔离；全代可解析后再逐 member 发布、删除 omitted、写 primary
    /// manifest、Committed marker、提升 previous、cleanup。
    fn roll_forward_commit(&self, dir: &Dir, journal: &DatasetJournal) -> Result<(), StorageError> {
        // 结构合法但语义矛盾（如伪造的新修订号）一律 InvalidJournal 隔离，且不移动健康
        // 的 primary 代——绝不据一条自相矛盾的 journal 发布 stage。
        self.validate_commit_journal_semantics(dir, journal)?;

        let primary_dir = PathBuf::from(PRIMARY_DIR);
        let stage_dir = PathBuf::from(format!(".stage-{}", journal.随机数));
        let stage_members = stage_dir.join(MEMBERS_DIR);
        let member_store = PathBuf::from(MEMBERS_DIR);

        dir.create_dir_all(&member_store).map_err(proto::map_io)?;
        for member in &journal.成员集合 {
            let stored_content = member_store.join(&member.摘要);
            if proto::exists(dir, &stored_content)? {
                continue;
            }
            let staged_content = stage_members.join(&member.摘要);
            let digest = proto::digest_file(dir, &staged_content)?;
            if digest.as_deref() != Some(member.摘要.as_str()) {
                return Err(self.quarantine_corrupt(
                    dir,
                    CorruptionReason::DatasetMemberDigestMismatch,
                    false,
                ));
            }
            dir.rename(&staged_content, dir, &stored_content)
                .map_err(proto::map_io)?;
        }
        proto::sync_subdir(dir, &stage_members)?;
        proto::sync_subdir(dir, &member_store)?;

        let record = manifest_record_from_journal(journal);
        proto::write_manifest(dir, &primary_dir, &record, &journal.随机数)?;
        proto::sync_subdir(dir, &primary_dir)?;
        proto::sync_dir(dir)?;
        self.validate_shared_generation(dir, &record)?;

        let committed = DatasetJournal {
            阶段: JournalPhase::Committed,
            ..journal.clone()
        };
        proto::write_journal(dir, &committed)?;
        proto::sync_dir(dir)?;

        self.finalize_previous_swap(dir)?;
        self.remove_legacy_generation_blobs(dir)?;
        proto::sync_dir(dir)?;
        let _ = dir.remove_dir_all(&stage_dir);
        proto::remove_journal(dir)?;
        self.remove_transients(dir)?;
        proto::sync_dir(dir)
    }

    /// 前滚一个「提升上一代」事务。交换用三次 rename 表达，可能在任意中间态崩溃；
    /// 此处据 primary/previous/swap 各自状态定位并前滚到底，绝不删除唯一副本。
    fn roll_forward_promote(
        &self,
        dir: &Dir,
        journal: &DatasetJournal,
    ) -> Result<(), StorageError> {
        // 结构合法但语义矛盾（old/new 修订号或成员摘要与磁盘上的 primary/previous 证据
        // 不符）一律 InvalidJournal 隔离，绝不执行 swap。
        self.validate_promote_journal_semantics(dir, journal)?;
        self.advance_promote_swap(dir, journal)?;
        proto::sync_dir(dir)?;
        self.remove_transients(dir)?;
        proto::remove_journal(dir)?;
        proto::sync_dir(dir)
    }

    /// 提交前滚前的语义校验：结构合法但语义矛盾（如伪造的新修订号）即拒绝。
    ///
    /// 1. 内部自洽：据 journal 记录的每个成员证据（名称 + 字节数 + 修订摘要）精确重算的
    ///    新修订号必须等于 journal 记录的新修订号。伪造的修订号绝不能据以发布 stage。
    /// 2. 旧证据一致：旧修订号必须与提交前的 primary manifest 一致；若成员已部分发布导致
    ///    primary manifest 已前进到新代，则由 previous.next manifest 提供旧证据。仅在能
    ///    明确证伪时拒绝（无任何旧证据时不武断否决）。
    ///
    /// 任一不符即 `InvalidJournal` 隔离，`include_primary=false`——绝不移动健康的
    /// primary 代。
    fn validate_commit_journal_semantics(
        &self,
        dir: &Dir,
        journal: &DatasetJournal,
    ) -> Result<(), StorageError> {
        let stage_members = PathBuf::from(format!(".stage-{}", journal.随机数)).join(MEMBERS_DIR);
        let shared_members = PathBuf::from(MEMBERS_DIR);
        let mut actual_evidence = Vec::with_capacity(journal.成员集合.len());
        for member in &journal.成员集合 {
            let (bytes, from_shared_store) =
                match proto::read_file(dir, &stage_members.join(&member.摘要))? {
                    Some(bytes) => (bytes, false),
                    None => (
                        proto::read_file(dir, &shared_members.join(&member.摘要))?.ok_or_else(
                            || {
                                self.quarantine_corrupt(
                                    dir,
                                    CorruptionReason::DatasetMemberDigestMismatch,
                                    false,
                                )
                            },
                        )?,
                        true,
                    ),
                };
            let revision_digest = revision_member_digest(&bytes);
            if proto::digest_bytes(&bytes) != member.摘要 {
                return Err(self.quarantine_corrupt(
                    dir,
                    CorruptionReason::DatasetMemberDigestMismatch,
                    from_shared_store,
                ));
            }
            if bytes.len() as u64 != member.字节数
                || proto::encode_revision(&revision_digest) != member.修订摘要
            {
                return Err(self.quarantine_corrupt(dir, CorruptionReason::InvalidJournal, false));
            }
            actual_evidence.push((member.名称.as_str(), bytes.len() as u64, revision_digest));
        }
        let recomputed = proto::encode_revision(
            DatasetRevision::from_member_digests(&actual_evidence).as_bytes(),
        );
        if recomputed != journal.新修订号 {
            return Err(self.quarantine_corrupt(dir, CorruptionReason::InvalidJournal, false));
        }
        let primary_rev = self.generation_revision(dir, Path::new(PRIMARY_DIR))?;
        let previous_next_rev = self.generation_revision(dir, Path::new(PREVIOUS_NEXT_DIR))?;
        let old = journal.旧修订号.as_str();
        let new = journal.新修订号.as_str();
        let old_ok = (primary_rev.is_none() && previous_next_rev.is_none())
            || primary_rev.as_deref() == Some(old)
            || previous_next_rev.as_deref() == Some(old)
            // 晚期恢复：primary manifest 已前进到新代，旧证据由 previous.next 提供。
            || primary_rev.as_deref() == Some(new);
        if !old_ok {
            return Err(self.quarantine_corrupt(dir, CorruptionReason::InvalidJournal, false));
        }
        Ok(())
    }

    /// 提升前滚前的语义校验：结构合法但语义矛盾即拒绝且绝不 swap。
    ///
    /// 1. 内部自洽：据成员证据重算的新代修订号必须等于 journal 新修订号。
    /// 2. 磁盘锚定：交换的任一崩溃中间态里，`primary`/`previous`/`.swap-<随机数>` 三个
    ///    槽位恰好承载 `{旧修订号, 新修订号}` 两代。据此确认 journal 的 old/new 修订号确实
    ///    对应现存的两代，且无第三方代混入。
    ///
    /// 任一不符即 `InvalidJournal` 隔离，`include_primary=false`——绝不移动 primary /
    /// previous 代，也不执行任何 rename。
    fn validate_promote_journal_semantics(
        &self,
        dir: &Dir,
        journal: &DatasetJournal,
    ) -> Result<(), StorageError> {
        let recomputed = recompute_revision_hex(&journal.成员集合)?;
        if recomputed != journal.新修订号 {
            return Err(self.quarantine_corrupt(dir, CorruptionReason::InvalidJournal, false));
        }
        let swap = PathBuf::from(format!(".swap-{}", journal.随机数));
        let generations = [
            PathBuf::from(PRIMARY_DIR),
            PathBuf::from(PREVIOUS_DIR),
            swap,
        ];
        let mut present: Vec<(PathBuf, String)> = Vec::new();
        for gen in generations {
            if let Some(rev) = self.generation_revision(dir, &gen)? {
                present.push((gen, rev));
            }
        }
        let old = journal.旧修订号.as_str();
        let new = journal.新修订号.as_str();
        let has_old = present.iter().any(|(_, rev)| rev == old);
        let has_new = present.iter().any(|(_, rev)| rev == new);
        let all_known = present.iter().all(|(_, rev)| rev == old || rev == new);
        if !(has_old && has_new && all_known) {
            return Err(self.quarantine_corrupt(dir, CorruptionReason::InvalidJournal, false));
        }
        let new_generation = present
            .iter()
            .find_map(|(path, revision)| (revision == new).then_some(path))
            .expect("has_new 已证明待提升代存在");
        self.validate_generation_against_journal(dir, new_generation, journal)?;
        Ok(())
    }

    /// 将待提升代的实际成员字节绑定到 Prepared journal，禁止仅凭 manifest/revision
    /// 元数据交换一个已被篡改的 generation。
    fn validate_generation_against_journal(
        &self,
        dir: &Dir,
        generation: &Path,
        journal: &DatasetJournal,
    ) -> Result<(), StorageError> {
        let Some(record) = proto::read_manifest(dir, generation)? else {
            return Err(self.quarantine_corrupt(
                dir,
                CorruptionReason::DatasetMemberDigestMismatch,
                generation == Path::new(PRIMARY_DIR),
            ));
        };
        let expected_names: Vec<&str> = journal
            .成员集合
            .iter()
            .map(|member| member.名称.as_str())
            .collect();
        let actual_names: Vec<&str> = record.成员集合.iter().map(String::as_str).collect();
        if actual_names != expected_names {
            return Err(self.quarantine_corrupt(
                dir,
                CorruptionReason::DatasetMemberDigestMismatch,
                generation == Path::new(PRIMARY_DIR),
            ));
        }
        for member in &journal.成员集合 {
            let member_record = record
                .成员证据
                .iter()
                .find(|record_member| record_member.名称 == member.名称)
                .ok_or_else(|| {
                    self.quarantine_corrupt(
                        dir,
                        CorruptionReason::DatasetMemberDigestMismatch,
                        generation == Path::new(PRIMARY_DIR),
                    )
                })?;
            let content_path = if member_record.内容摘要.is_empty() {
                generation.join(BLOBS_DIR).join(&member.名称)
            } else {
                PathBuf::from(MEMBERS_DIR).join(&member_record.内容摘要)
            };
            let Some(bytes) = proto::read_file(dir, &content_path)? else {
                return Err(self.quarantine_corrupt(
                    dir,
                    CorruptionReason::DatasetMemberDigestMismatch,
                    generation == Path::new(PRIMARY_DIR),
                ));
            };
            if proto::digest_bytes(&bytes) != member.摘要
                || bytes.len() as u64 != member.字节数
                || proto::revision_member_digest_hex(&bytes) != member.修订摘要
            {
                return Err(self.quarantine_corrupt(
                    dir,
                    CorruptionReason::DatasetMemberDigestMismatch,
                    generation == Path::new(PRIMARY_DIR),
                ));
            }
        }
        Ok(())
    }
    /// 无 journal 时清理 pre-Prepared 残留（stage / swap / journal 临时文件 /
    /// 悬挂的 previous.next）。这些残留没有跨越逻辑提交点，安全丢弃。
    fn cleanup_orphans(&self, dir: &Dir) -> Result<(), StorageError> {
        let removed = self.remove_transients(dir)?;
        let previous_next = PathBuf::from(PREVIOUS_NEXT_DIR);
        let mut removed_next = false;
        if proto::exists(dir, &previous_next)? {
            dir.remove_dir_all(&previous_next).map_err(proto::map_io)?;
            removed_next = true;
        }
        if removed || removed_next {
            proto::sync_dir(dir)?;
        }
        Ok(())
    }

    /// 移除所有 `.stage-*` / `.swap-*` / `.journal-*` 临时项；返回是否删除过任何项。
    /// 已隔离的 `.corrupt.*` 证据绝不删除。
    fn remove_transients(&self, dir: &Dir) -> Result<bool, StorageError> {
        let mut transient: Vec<OsString> = Vec::new();
        for entry in dir.entries().map_err(proto::map_io)? {
            let name = entry.map_err(proto::map_io)?.file_name();
            let display = name.to_string_lossy();
            if display.contains(".corrupt.") {
                continue;
            }
            if display.starts_with(".stage-")
                || display.starts_with(".swap-")
                || display.starts_with(".journal-")
            {
                transient.push(name);
            }
        }
        let removed = !transient.is_empty();
        for name in transient {
            if dir.remove_dir_all(&name).is_err() {
                let _ = dir.remove_file(&name);
            }
        }
        Ok(removed)
    }

    fn finalize_previous_swap(&self, dir: &Dir) -> Result<(), StorageError> {
        let next = PathBuf::from(PREVIOUS_NEXT_DIR);
        if proto::exists(dir, &next)? {
            let previous = PathBuf::from(PREVIOUS_DIR);
            if proto::exists(dir, &previous)? {
                dir.remove_dir_all(&previous).map_err(proto::map_io)?;
            }
            dir.rename(&next, dir, &previous).map_err(proto::map_io)?;
        }
        Ok(())
    }

    /// fail-closed：把事务证据移动到 `.corrupt.*` 隔离名下，并返回带处置的 typed 事务
    /// 损坏错误。
    ///
    /// 搬运顺序被刻意固定为「journal 最先 → 其余事务残留 → primary 最后」：
    /// - journal 最先隔离会向任何观察者泄露本次关联的 `<id>`，也让我们在 primary 隔离
    ///   失败时能把 journal 原位复原为持久屏障。
    /// - `include_primary` 为 true 时（已发布的矛盾代自身即证据）最后才尝试隔离整个
    ///   `primary` 代；一旦无法隔离（如其损坏目标被占用），落盘并 fsync `corruption.marker`
    ///   并复原 journal——两者共同令后续恢复持续 fail-closed，绝不再打开仍在原位的矛盾
    ///   数据。为 false 时（如 InvalidJournal）绝不移动健康的 primary 代。
    fn quarantine_corrupt(
        &self,
        dir: &Dir,
        reason: CorruptionReason,
        include_primary: bool,
    ) -> StorageError {
        let id = Uuid::new_v4().simple().to_string();
        let mut disposition = QuarantineDisposition::EvidenceQuarantined;

        // 收集非 primary 的事务证据；journal 单列以保证它最先被搬运。
        let mut journal_present = false;
        let mut evidence: Vec<OsString> = Vec::new();
        match dir.entries() {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let display = name.to_string_lossy();
                    if display.contains(".corrupt.") {
                        continue;
                    }
                    if display == JOURNAL_FILE {
                        journal_present = true;
                        continue;
                    }
                    let is_evidence = display == PREVIOUS_NEXT_DIR
                        || display.starts_with(".stage-")
                        || display.starts_with(".swap-")
                        || display.starts_with(".journal-");
                    if is_evidence {
                        evidence.push(name);
                    }
                }
            }
            Err(_) => disposition = QuarantineDisposition::QuarantineFailed,
        }

        // 在搬运任何证据前先建立并持久化 fail-closed 屏障。若屏障无法建立，
        // active journal 必须保持原位，不能制造“无 journal 且 primary 仍活跃”的窗口。
        if let Err(error) = self.write_corruption_marker(dir) {
            return StorageError::new(
                StorageErrorKind::CorruptTransaction(CorruptTransactionError::new(
                    TransactionScope::Dataset,
                    reason,
                    QuarantineDisposition::QuarantineFailed,
                )),
                format!("数据集损坏屏障无法持久化：{error}"),
            );
        }

        // journal 搬运发生前 marker 已 durable，任意后续 crash 都持续 fail-closed。
        let journal_target = PathBuf::from(format!("{JOURNAL_FILE}.corrupt.{id}"));
        let mut journal_quarantined = false;
        if journal_present {
            match Self::quarantine_rename(dir, Path::new(JOURNAL_FILE), &journal_target) {
                Ok(true) => journal_quarantined = true,
                Ok(false) => {}
                Err(()) => disposition = QuarantineDisposition::QuarantineFailed,
            }
        }

        // 其余事务残留。
        for name in &evidence {
            let target = PathBuf::from(format!("{}.corrupt.{id}", name.to_string_lossy()));
            if Self::quarantine_rename(dir, Path::new(name), &target).is_err() {
                disposition = QuarantineDisposition::QuarantineFailed;
            }
        }

        // 最后才尝试隔离矛盾 primary 代。
        if include_primary {
            let primary_target = PathBuf::from(format!("{PRIMARY_DIR}.corrupt.{id}"));
            let isolated =
                Self::quarantine_rename(dir, Path::new(PRIMARY_DIR), &primary_target).is_ok();
            if !isolated {
                // marker 已在任何搬运前 durable；尝试复原 journal 以保留额外证据。
                disposition = QuarantineDisposition::QuarantineFailed;
                if journal_quarantined
                    && dir
                        .rename(&journal_target, dir, Path::new(JOURNAL_FILE))
                        .is_err()
                {
                    disposition = QuarantineDisposition::QuarantineFailed;
                }
            }
        }

        // 隔离完整成功后，将 journal 从“未完成隔离屏障”名称转为普通证据名称。
        // marker 仍存在时先 fsync 所有隔离 rename；只有证明证据位置 durable 后才删除
        // marker，并再次 fsync marker 删除。任一失败都保留屏障、继续 fail-closed。
        if disposition == QuarantineDisposition::EvidenceQuarantined {
            if journal_quarantined {
                let evidence_target = PathBuf::from(format!("transaction.corrupt.{id}"));
                if dir.rename(&journal_target, dir, &evidence_target).is_err() {
                    disposition = QuarantineDisposition::QuarantineFailed;
                }
            }
            if disposition == QuarantineDisposition::EvidenceQuarantined
                && proto::sync_dir(dir).is_err()
            {
                disposition = QuarantineDisposition::QuarantineFailed;
            }
            if disposition == QuarantineDisposition::EvidenceQuarantined
                && (dir.remove_file(CORRUPTION_MARKER).is_err() || proto::sync_dir(dir).is_err())
            {
                disposition = QuarantineDisposition::QuarantineFailed;
            }
        }
        let _ = proto::sync_dir(dir);
        StorageError::new(
            StorageErrorKind::CorruptTransaction(CorruptTransactionError::new(
                TransactionScope::Dataset,
                reason,
                disposition,
            )),
            "数据集事务证据矛盾，已 fail-closed",
        )
    }
    /// 把单个证据条目搬到其 `.corrupt.<id>` 隔离名下（拒绝符号链接）。
    /// - `Ok(true)`：已搬运；`Ok(false)`：源已不存在（竞态），无需搬运；
    /// - `Err(())`：符号链接 / 元数据错误 / rename 失败，隔离未达成。
    fn quarantine_rename(dir: &Dir, source: &Path, target: &Path) -> Result<bool, ()> {
        match dir.symlink_metadata(source) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(()),
        }
        dir.rename(source, dir, target)
            .map(|()| true)
            .map_err(|_| ())
    }

    /// 在隔离任何证据前落盘并 fsync 持久损坏标记。已存在即视为屏障就位。
    fn write_corruption_marker(&self, dir: &Dir) -> Result<(), StorageError> {
        if !proto::exists(dir, Path::new(CORRUPTION_MARKER))? {
            proto::write_new_file(
                dir,
                Path::new(CORRUPTION_MARKER),
                b"aemeath dataset transaction corruption\n",
            )?;
        }
        proto::sync_dir(dir)
    }
    // ---- 读取辅助 ----

    fn current_manifest(&self, dir: &Dir) -> Result<DatasetManifest, StorageError> {
        match proto::read_manifest(dir, Path::new(PRIMARY_DIR))? {
            Some(record) => manifest_from_record(dir, PRIMARY_DIR, &record),
            None => DatasetManifest::new(Vec::new()),
        }
    }

    fn read_generation(
        &self,
        dir: &Dir,
        generation_dir: &str,
        members: &[SafePathSegment],
    ) -> Result<DatasetReadOutcome, StorageError> {
        let generation_path = PathBuf::from(generation_dir);
        let Some(record) = proto::read_manifest(dir, &generation_path)? else {
            return Ok(DatasetReadOutcome::NotFound);
        };
        let revision = DatasetRevision::from_bytes(proto::decode_revision(&record.修订号)?);
        let legacy_blobs = generation_path.join(BLOBS_DIR);
        let uses_legacy_member_layout = !record.成员集合.is_empty()
            && (record.成员证据.is_empty()
                || record
                    .成员证据
                    .iter()
                    .all(|member| member.内容摘要.is_empty()));
        let mut result = Vec::with_capacity(members.len());
        for name in members {
            let content_path = if uses_legacy_member_layout {
                if !record
                    .成员集合
                    .iter()
                    .any(|member_name| member_name == name.as_str())
                {
                    return Ok(DatasetReadOutcome::NotFound);
                }
                legacy_blobs.join(name.as_str())
            } else {
                let Some(member_record) = record
                    .成员证据
                    .iter()
                    .find(|member| member.名称 == name.as_str())
                else {
                    return Ok(DatasetReadOutcome::NotFound);
                };
                PathBuf::from(MEMBERS_DIR).join(&member_record.内容摘要)
            };
            let Some(bytes) = proto::read_file(dir, &content_path)? else {
                return Ok(DatasetReadOutcome::NotFound);
            };
            if !uses_legacy_member_layout
                && proto::digest_bytes(&bytes)
                    != record
                        .成员证据
                        .iter()
                        .find(|member| member.名称 == name.as_str())
                        .map(|member| member.内容摘要.as_str())
                        .unwrap_or_default()
            {
                return Err(dataset_member_mismatch());
            }
            result.push(DatasetMember::new(name.clone(), bytes));
        }
        Ok(DatasetReadOutcome::Found(DatasetRead::new(
            revision, result,
        )?))
    }

    // ---- 提交（CAS 先行；Prepared 落盘为逻辑提交点） ----

    fn commit_incremental_sync(
        &self,
        dir: &Dir,
        changes: &DatasetChangeSet,
    ) -> Result<DatasetCommitReceipt, StorageError> {
        let current = self.current_manifest(dir)?;
        if current.revision() != changes.expected_revision() {
            return Err(StorageError::new(
                StorageErrorKind::ConcurrentWrite,
                "数据集修订号已变更，增量提交被拒绝",
            ));
        }

        let current_names = current.members().iter().collect::<BTreeSet<_>>();
        let removed_names = changes.removed_members().iter().collect::<BTreeSet<_>>();
        if changes
            .removed_members()
            .iter()
            .any(|name| !current_names.contains(name))
        {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "待删除的数据集成员不存在于期望代",
            ));
        }

        let primary_blobs = PathBuf::from(PRIMARY_DIR).join(BLOBS_DIR);
        let current_record = proto::read_manifest(dir, Path::new(PRIMARY_DIR))?;
        let current_members_by_name = current_record
            .as_ref()
            .into_iter()
            .flat_map(|record| record.成员证据.iter())
            .map(|member| (member.名称.as_str(), member))
            .collect::<std::collections::BTreeMap<_, _>>();
        let uses_legacy_member_layout = current_record
            .as_ref()
            .is_some_and(|record| record.成员证据.is_empty() && !record.成员集合.is_empty());
        let mut reused_content = Vec::with_capacity(changes.reused_members().len());
        let member_store = PathBuf::from(MEMBERS_DIR);
        dir.create_dir_all(&member_store).map_err(proto::map_io)?;
        for reference in changes.reused_members() {
            if !current_names.contains(reference.name()) || removed_names.contains(reference.name())
            {
                return Err(dataset_member_mismatch());
            }
            let content_path = if uses_legacy_member_layout {
                primary_blobs.join(reference.name().as_str())
            } else {
                let member_record = current_members_by_name
                    .get(reference.name().as_str())
                    .copied()
                    .ok_or_else(dataset_member_mismatch)?;
                if member_record.内容摘要.is_empty() {
                    primary_blobs.join(reference.name().as_str())
                } else {
                    PathBuf::from(MEMBERS_DIR).join(&member_record.内容摘要)
                }
            };
            let bytes =
                proto::read_file(dir, &content_path)?.ok_or_else(dataset_member_mismatch)?;
            if bytes.len() as u64 != reference.byte_len()
                || revision_member_digest(&bytes) != *reference.member_digest()
            {
                return Err(dataset_member_mismatch());
            }
            let content_digest = proto::digest_bytes(&bytes);
            let stored_content = member_store.join(&content_digest);
            if !proto::exists(dir, &stored_content)? {
                proto::write_new_file(dir, &stored_content, &bytes)?;
            }
            reused_content.push((reference.name().clone(), content_digest));
        }

        proto::sync_subdir(dir, &member_store)?;

        let target_names = changes
            .new_members()
            .iter()
            .map(|change| change.member().name())
            .chain(
                changes
                    .reused_members()
                    .iter()
                    .map(|reference| reference.name()),
            )
            .collect::<BTreeSet<_>>();
        if current
            .members()
            .iter()
            .any(|name| !target_names.contains(name) && !removed_names.contains(name))
        {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "增量提交必须明确复用、替换或删除每个当前成员",
            ));
        }

        self.commit_incremental_verified(dir, &current, changes, &reused_content)
    }

    fn commit_incremental_verified(
        &self,
        dir: &Dir,
        current: &DatasetManifest,
        changes: &DatasetChangeSet,
        reused_content: &[(SafePathSegment, String)],
    ) -> Result<DatasetCommitReceipt, StorageError> {
        let current_record = proto::read_manifest(dir, Path::new(PRIMARY_DIR))?;
        let new_members = changes
            .new_members()
            .iter()
            .map(|change| change.member())
            .collect::<Vec<_>>();
        let mut revision_evidence = new_members
            .iter()
            .map(|member| {
                (
                    member.name().as_str(),
                    member.bytes().len() as u64,
                    revision_member_digest(member.bytes()),
                )
            })
            .chain(changes.reused_members().iter().map(|reference| {
                (
                    reference.name().as_str(),
                    reference.byte_len(),
                    *reference.member_digest(),
                )
            }))
            .collect::<Vec<_>>();
        revision_evidence.sort_by(|left, right| left.0.cmp(right.0));
        let new_revision = DatasetRevision::from_member_digests(&revision_evidence);
        let member_names = revision_evidence
            .iter()
            .map(|(name, _, _)| SafePathSegment::from_str(name))
            .collect::<Result<Vec<_>, _>>()?;
        let member_evidence = revision_evidence
            .iter()
            .zip(member_names)
            .map(|((_, byte_len, member_digest), name)| {
                DatasetMemberReference::from_manifest_member(
                    new_revision.clone(),
                    name,
                    *byte_len,
                    *member_digest,
                )
            })
            .collect::<Vec<_>>();
        let _new_manifest =
            DatasetManifest::from_verified_members(new_revision.clone(), member_evidence)?;
        let nonce = Uuid::new_v4().simple().to_string();
        let mut journal_members = new_members
            .iter()
            .map(|member| JournalMember {
                名称: member.name().as_str().to_string(),
                摘要: proto::digest_bytes(member.bytes()),
                字节数: member.bytes().len() as u64,
                修订摘要: proto::revision_member_digest_hex(member.bytes()),
            })
            .collect::<Vec<_>>();
        for reference in changes.reused_members() {
            let digest = reused_content
                .iter()
                .find(|(name, _)| name == reference.name())
                .map(|(_, digest)| digest.clone())
                .ok_or_else(dataset_member_mismatch)?;
            journal_members.push(JournalMember {
                名称: reference.name().as_str().to_string(),
                摘要: digest,
                字节数: reference.byte_len(),
                修订摘要: proto::encode_revision(reference.member_digest()),
            });
        }
        journal_members.sort_by(|left, right| left.名称.cmp(&right.名称));
        let record = DatasetManifestRecord {
            修订号: proto::encode_revision(new_revision.as_bytes()),
            成员集合: journal_members
                .iter()
                .map(|member| member.名称.clone())
                .collect(),
            成员证据: journal_members
                .iter()
                .map(|member| DatasetManifestMemberRecord {
                    名称: member.名称.clone(),
                    字节数: member.字节数,
                    修订摘要: member.修订摘要.clone(),
                    内容摘要: member.摘要.clone(),
                })
                .collect(),
        };
        let journal = DatasetJournal {
            随机数: nonce.clone(),
            操作: JournalKind::Commit,
            旧修订号: proto::encode_revision(current.revision().as_bytes()),
            新修订号: proto::encode_revision(new_revision.as_bytes()),
            成员集合: journal_members,
            阶段: JournalPhase::Prepared,
        };
        let existing_content_by_digest = current_record
            .as_ref()
            .into_iter()
            .flat_map(|record| record.成员证据.iter())
            .filter(|member| !member.内容摘要.is_empty())
            .map(|member| member.内容摘要.as_str())
            .collect::<BTreeSet<_>>();
        let stage_dir = PathBuf::from(format!(".stage-{nonce}"));
        let stage_members = stage_dir.join(MEMBERS_DIR);
        let primary_dir = PathBuf::from(PRIMARY_DIR);

        let mut crossed_commit = false;
        let result = (|| -> Result<(), StorageError> {
            dir.create_dir_all(&stage_members).map_err(proto::map_io)?;
            for member in &new_members {
                let content_digest = proto::digest_bytes(member.bytes());
                let stored_content = PathBuf::from(MEMBERS_DIR).join(&content_digest);
                if !proto::exists(dir, &stored_content)?
                    && !existing_content_by_digest.contains(content_digest.as_str())
                {
                    proto::write_new_file(
                        dir,
                        &stage_members.join(content_digest),
                        member.bytes(),
                    )?;
                }
            }
            proto::write_manifest(dir, &stage_dir, &record, &nonce)?;
            proto::sync_subdir(dir, &stage_members)?;
            proto::sync_subdir(dir, &stage_dir)?;
            proto::sync_dir(dir)?;

            self.stage_previous_manifest(dir)?;
            if !proto::exists(dir, &primary_dir)? {
                dir.create_dir_all(&primary_dir).map_err(proto::map_io)?;
            }
            proto::write_journal(dir, &journal)?;
            proto::sync_dir(dir)?;
            crossed_commit = true;
            inject_post_prepared_fault(&FaultPoint::AfterPrepared)?;

            let member_store = PathBuf::from(MEMBERS_DIR);
            dir.create_dir_all(&member_store).map_err(proto::map_io)?;
            for member in &new_members {
                let content_digest = proto::digest_bytes(member.bytes());
                let staged_content = stage_members.join(&content_digest);
                let stored_content = member_store.join(&content_digest);
                if !proto::exists(dir, &stored_content)? {
                    dir.rename(&staged_content, dir, &stored_content)
                        .map_err(proto::map_io)?;
                    proto::sync_subdir(dir, &stage_members)?;
                    proto::sync_subdir(dir, &member_store)?;
                }
                inject_post_prepared_fault(&FaultPoint::AfterMemberPublish(
                    member.name().as_str().to_string(),
                ))?;
            }
            proto::write_manifest(dir, &primary_dir, &record, &nonce)?;
            proto::sync_subdir(dir, &primary_dir)?;
            proto::sync_dir(dir)?;
            self.validate_shared_generation(dir, &record)?;
            proto::write_journal(
                dir,
                &DatasetJournal {
                    阶段: JournalPhase::Committed,
                    ..journal.clone()
                },
            )?;
            proto::sync_dir(dir)?;
            self.finalize_previous_swap(dir)?;
            self.remove_legacy_generation_blobs(dir)?;
            proto::sync_dir(dir)?;
            let _ = dir.remove_dir_all(&stage_dir);
            proto::remove_journal(dir)?;
            // 提交收尾：previous 前进后，旧 previous 代独有内容已成孤儿，立即回收。
            self.collect_orphan_member_content_best_effort(dir);
            proto::sync_dir(dir)
        })();

        match result {
            Ok(()) => Ok(DatasetCommitReceipt::committed(
                new_revision,
                DatasetCommitVisibility::Visible,
                None,
            )),
            Err(error) if matches!(error.kind(), StorageErrorKind::CorruptTransaction(_)) => {
                Err(error)
            }
            Err(_) if crossed_commit => {
                log::warn!(target: crate::LOG_TARGET, "dataset_commit recovery_pending");
                Ok(DatasetCommitReceipt::committed(
                    new_revision,
                    DatasetCommitVisibility::RecoveryPending,
                    Some(CommitWarning::MemberPublishRecoveryPending),
                ))
            }
            Err(error) => {
                let _ = dir.remove_dir_all(&stage_dir);
                let _ = dir.remove_dir_all(PathBuf::from(PREVIOUS_NEXT_DIR));
                Err(error)
            }
        }
    }

    fn stage_previous_manifest(&self, dir: &Dir) -> Result<(), StorageError> {
        let previous_next = PathBuf::from(PREVIOUS_NEXT_DIR);
        if proto::exists(dir, &previous_next)? {
            dir.remove_dir_all(&previous_next).map_err(proto::map_io)?;
            proto::sync_dir(dir)?;
        }
        let primary_manifest = PathBuf::from(PRIMARY_DIR).join(MANIFEST_FILE);
        let Some(bytes) = proto::read_file(dir, &primary_manifest)? else {
            return proto::sync_dir(dir);
        };
        dir.create_dir_all(&previous_next).map_err(proto::map_io)?;
        proto::write_new_file(dir, &previous_next.join(MANIFEST_FILE), &bytes)?;
        proto::sync_subdir(dir, &previous_next)?;
        proto::sync_dir(dir)
    }

    fn remove_legacy_generation_blobs(&self, dir: &Dir) -> Result<(), StorageError> {
        for generation in [PRIMARY_DIR, PREVIOUS_DIR] {
            let blobs = PathBuf::from(generation).join(BLOBS_DIR);
            if proto::exists(dir, &blobs)? {
                dir.remove_dir_all(&blobs).map_err(proto::map_io)?;
            }
        }
        Ok(())
    }

    fn validate_shared_generation(
        &self,
        dir: &Dir,
        record: &DatasetManifestRecord,
    ) -> Result<(), StorageError> {
        for member in &record.成员证据 {
            if member.内容摘要.is_empty() {
                return Err(dataset_member_mismatch());
            }
            let bytes = proto::read_file(dir, &PathBuf::from(MEMBERS_DIR).join(&member.内容摘要))?
                .ok_or_else(dataset_member_mismatch)?;
            if proto::digest_bytes(&bytes) != member.内容摘要
                || bytes.len() as u64 != member.字节数
                || proto::revision_member_digest_hex(&bytes) != member.修订摘要
            {
                return Err(dataset_member_mismatch());
            }
        }
        Ok(())
    }

    fn commit_sync(
        &self,
        dir: &Dir,
        expected: &DatasetRevision,
        members: &[DatasetMember],
    ) -> Result<DatasetCommitReceipt, StorageError> {
        let current = self.current_manifest(dir)?;
        if current.revision() != expected {
            return Err(StorageError::new(
                StorageErrorKind::ConcurrentWrite,
                "数据集修订号已变更，提交被拒绝",
            ));
        }
        let replacements = members
            .iter()
            .cloned()
            .map(DatasetMemberChange::Replace)
            .collect();
        let target_names = members
            .iter()
            .map(|member| member.name().clone())
            .collect::<BTreeSet<_>>();
        let removed_members = current
            .members()
            .iter()
            .filter(|name| !target_names.contains(*name))
            .cloned()
            .collect();
        let changes = DatasetChangeSet::new(expected.clone(), replacements, Vec::new())?
            .with_removed_members(removed_members)?;
        self.commit_incremental_sync(dir, &changes)
    }

    /// 提升前完整验证上一代：每个成员字节必须存在，且据其重算的
    /// `DatasetManifest` 修订号必须与持久化记录相符。任一缺失/矛盾即返回 typed
    /// `DatasetMemberDigestMismatch`，调用方据此在写 journal 之前拒绝提升。
    fn validate_previous_generation(
        &self,
        dir: &Dir,
        record: &DatasetManifestRecord,
    ) -> Result<(), StorageError> {
        let mut members = Vec::with_capacity(record.成员集合.len());
        for name in &record.成员集合 {
            let segment = SafePathSegment::from_str(name)?;
            let member_record = record
                .成员证据
                .iter()
                .find(|member| member.名称 == *name)
                .ok_or_else(dataset_member_mismatch)?;
            let content_path = if member_record.内容摘要.is_empty() {
                PathBuf::from(PREVIOUS_DIR).join(BLOBS_DIR).join(name)
            } else {
                PathBuf::from(MEMBERS_DIR).join(&member_record.内容摘要)
            };
            let bytes =
                proto::read_file(dir, &content_path)?.ok_or_else(dataset_member_mismatch)?;
            if !member_record.内容摘要.is_empty()
                && proto::digest_bytes(&bytes) != member_record.内容摘要
            {
                return Err(dataset_member_mismatch());
            }
            members.push(DatasetMember::new(segment, bytes));
        }
        let recomputed = DatasetManifest::new(members)?;
        let expected = DatasetRevision::from_bytes(proto::decode_revision(&record.修订号)?);
        if recomputed.revision() != &expected {
            return Err(dataset_member_mismatch());
        }
        Ok(())
    }

    fn promote_sync(&self, dir: &Dir) -> Result<DatasetCommitReceipt, StorageError> {
        let previous_dir = PathBuf::from(PREVIOUS_DIR);
        let Some(previous_record) = proto::read_manifest(dir, &previous_dir)? else {
            // 无上一代可提升：无操作，回报当前 primary 修订号。
            let current = self.current_manifest(dir)?;
            return Ok(DatasetCommitReceipt::committed(
                current.revision().clone(),
                DatasetCommitVisibility::Visible,
                None,
            ));
        };
        // D：跨越 Prepared 之前完整验证上一代，缺失/矛盾则 typed 拒绝且绝不写 journal。
        self.validate_previous_generation(dir, &previous_record)?;

        let new_revision =
            DatasetRevision::from_bytes(proto::decode_revision(&previous_record.修订号)?);
        let current = self.current_manifest(dir)?;

        let mut journal_members = Vec::with_capacity(previous_record.成员集合.len());
        for member_record in &previous_record.成员证据 {
            let content_path = if member_record.内容摘要.is_empty() {
                previous_dir.join(BLOBS_DIR).join(&member_record.名称)
            } else {
                PathBuf::from(MEMBERS_DIR).join(&member_record.内容摘要)
            };
            let bytes =
                proto::read_file(dir, &content_path)?.ok_or_else(dataset_member_mismatch)?;
            journal_members.push(JournalMember {
                名称: member_record.名称.clone(),
                摘要: proto::digest_bytes(&bytes),
                字节数: bytes.len() as u64,
                修订摘要: proto::revision_member_digest_hex(&bytes),
            });
        }
        // journal 成员集合须按名称严格升序（recover 前的结构校验要求）。
        journal_members.sort_by(|left, right| left.名称.cmp(&right.名称));

        let nonce = Uuid::new_v4().simple().to_string();
        let journal = DatasetJournal {
            随机数: nonce.clone(),
            操作: JournalKind::PromotePrevious,
            旧修订号: proto::encode_revision(current.revision().as_bytes()),
            新修订号: proto::encode_revision(new_revision.as_bytes()),
            成员集合: journal_members,
            阶段: JournalPhase::Prepared,
        };

        let mut crossed_commit = false;
        let result = (|| -> Result<(), StorageError> {
            // 逻辑提交点：Prepared journal 落盘。
            proto::write_journal(dir, &journal)?;
            proto::sync_dir(dir)?;
            crossed_commit = true;
            inject_post_prepared_fault(&FaultPoint::PromoteAfterPrepared)?;

            // 前滚三次 rename 交换（含中途故障点）。
            self.advance_promote_swap(dir, &journal)?;

            // Committed marker + cleanup。
            let committed = DatasetJournal {
                阶段: JournalPhase::Committed,
                ..journal.clone()
            };
            proto::write_journal(dir, &committed)?;
            proto::sync_dir(dir)?;
            proto::remove_journal(dir)?;
            proto::sync_dir(dir)?;
            Ok(())
        })();

        match result {
            Ok(()) => Ok(DatasetCommitReceipt::committed(
                new_revision,
                DatasetCommitVisibility::Visible,
                None,
            )),
            Err(error) => {
                if matches!(error.kind(), StorageErrorKind::CorruptTransaction(_)) {
                    return Err(error);
                }
                if crossed_commit {
                    // 越过 Prepared：普通 post-Prepared 故障返回 committed 收据，
                    // 交由后续锁内恢复机械前滚，绝不上抛 Err。
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "dataset_promote recovery_pending"
                    );
                    Ok(DatasetCommitReceipt::committed(
                        new_revision,
                        DatasetCommitVisibility::RecoveryPending,
                        Some(CommitWarning::MemberPublishRecoveryPending),
                    ))
                } else {
                    let _ = proto::remove_journal(dir);
                    Err(error)
                }
            }
        }
    }

    /// 可续跑的提升交换状态机：据 `primary` / `previous` / `.swap-<随机数>` 的当前
    /// 状态定位并前滚到底，目标是 `Primary = 旧 Previous`、`Previous = 旧 Primary`。
    ///
    /// 崩溃状态与前滚：
    /// - 无 swap 且 primary 修订号 == 新修订号（目标）：已完成，仅收尾。
    /// - 无 swap 且 primary 修订号 == 旧修订号：Prepared 之后尚未换。执行
    ///   `primary→swap`（如存在）、`previous→primary`、`swap→previous`。
    /// - 有 swap 且 primary 存在：`previous→primary` 已完成，仅剩 `swap→previous`。
    /// - 有 swap 且 primary 缺失：`primary→swap` 已完成，执行 `previous→primary`、
    ///   `swap→previous`。
    ///
    /// 全程只用 rename，任意时刻两代各存在一份，绝不删除唯一副本。故障注入点仅在
    /// 设置了测试环境变量时才触发，恢复路径（无环境变量）无害地全部跳过。
    fn advance_promote_swap(
        &self,
        dir: &Dir,
        journal: &DatasetJournal,
    ) -> Result<(), StorageError> {
        let primary_dir = PathBuf::from(PRIMARY_DIR);
        let previous_dir = PathBuf::from(PREVIOUS_DIR);
        let swap = PathBuf::from(format!(".swap-{}", journal.随机数));
        let target_rev = journal.新修订号.as_str();

        let swap_exists = proto::exists(dir, &swap)?;
        let primary_exists = proto::exists(dir, &primary_dir)?;

        if !swap_exists {
            let primary_rev = self.generation_revision(dir, &primary_dir)?;
            if primary_rev.as_deref() == Some(target_rev) {
                // 已完成交换，仅收尾。
                return Ok(());
            }
            // Prepared 之后尚未开始换：primary→swap（如存在）。
            if primary_exists {
                proto::reject_symlink(dir, &swap)?;
                dir.rename(&primary_dir, dir, &swap)
                    .map_err(proto::map_io)?;
                proto::sync_dir(dir)?;
            }
            inject_post_prepared_fault(&FaultPoint::PromoteAfterPrimaryToSwap)?;
            // previous→primary。
            dir.rename(&previous_dir, dir, &primary_dir)
                .map_err(proto::map_io)?;
            proto::sync_dir(dir)?;
            inject_post_prepared_fault(&FaultPoint::PromoteAfterPreviousToPrimary)?;
            // swap→previous（如存在）。
            if proto::exists(dir, &swap)? {
                dir.rename(&swap, dir, &previous_dir)
                    .map_err(proto::map_io)?;
                proto::sync_dir(dir)?;
            }
            return Ok(());
        }

        // 有 swap：换已开始。
        if primary_exists {
            // previous→primary 已完成，仅剩 swap→previous。
            dir.rename(&swap, dir, &previous_dir)
                .map_err(proto::map_io)?;
            proto::sync_dir(dir)?;
        } else {
            // 仅完成 primary→swap：补做 previous→primary、swap→previous。
            dir.rename(&previous_dir, dir, &primary_dir)
                .map_err(proto::map_io)?;
            proto::sync_dir(dir)?;
            inject_post_prepared_fault(&FaultPoint::PromoteAfterPreviousToPrimary)?;
            dir.rename(&swap, dir, &previous_dir)
                .map_err(proto::map_io)?;
            proto::sync_dir(dir)?;
        }
        Ok(())
    }

    /// 读取某代 manifest 的持久化修订号（十六进制字符串）；代缺失返回 `None`。
    fn generation_revision(
        &self,
        dir: &Dir,
        gen_dir: &Path,
    ) -> Result<Option<String>, StorageError> {
        Ok(proto::read_manifest(dir, gen_dir)?.map(|record| record.修订号))
    }

    fn quarantine_sync(
        &self,
        dir: &Dir,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcome, StorageError> {
        let generation_dir = match generation {
            Generation::Primary => PathBuf::from(PRIMARY_DIR),
            Generation::Previous => PathBuf::from(PREVIOUS_DIR),
        };
        if !proto::exists(dir, &generation_dir.join(MANIFEST_FILE))? {
            return Ok(QuarantineOutcome::already_absent(generation, scope, reason));
        }
        proto::reject_symlink(dir, &generation_dir)?;
        let id = SafePathSegment::from_str(&Uuid::new_v4().simple().to_string())?;
        let target = PathBuf::from(format!(
            "{}.quarantine.{}",
            generation_dir.to_string_lossy(),
            id.as_str()
        ));
        dir.rename(&generation_dir, dir, &target)
            .map_err(proto::map_io)?;
        proto::sync_dir(dir)?;
        Ok(QuarantineOutcome::Moved(QuarantineReceipt::new(
            id, generation, scope, reason,
        )))
    }
}

/// 据 journal 记录的成员证据（名称 + 字节数 + 修订摘要）精确重算修订号（十六进制）。
///
/// 无需原始字节即可复算，令恢复能语义校验 journal 记录的修订号是否自洽。
fn recompute_revision_hex(members: &[JournalMember]) -> Result<String, StorageError> {
    let mut digests: Vec<[u8; 32]> = Vec::with_capacity(members.len());
    for member in members {
        digests.push(proto::decode_revision(&member.修订摘要)?);
    }
    let evidence: Vec<(&str, u64, [u8; 32])> = members
        .iter()
        .zip(digests.iter())
        .map(|(member, digest)| (member.名称.as_str(), member.字节数, *digest))
        .collect();
    Ok(proto::encode_revision(
        DatasetRevision::from_member_digests(&evidence).as_bytes(),
    ))
}

fn corrupt_journal_barrier_error() -> StorageError {
    StorageError::new(
        StorageErrorKind::CorruptTransaction(CorruptTransactionError::new(
            TransactionScope::Dataset,
            CorruptionReason::InvalidJournal,
            QuarantineDisposition::EvidenceQuarantined,
        )),
        "数据集存在已隔离但未显式处置的事务日志，fail-closed",
    )
}

/// 存在持久损坏标记时的 typed 事务损坏错误：恢复入口据此持续 fail-closed。
fn corruption_marker_error() -> StorageError {
    StorageError::new(
        StorageErrorKind::CorruptTransaction(CorruptTransactionError::new(
            TransactionScope::Dataset,
            CorruptionReason::DatasetMemberDigestMismatch,
            QuarantineDisposition::QuarantineFailed,
        )),
        "数据集存在持久损坏标记，fail-closed",
    )
}

/// 上一代不完整或与其持久化记录矛盾时返回的 typed 事务损坏错误。
///
/// 用于「提升前完整验证上一代」失败：不移动任何证据，仅上抛 typed 错误，
/// 调用方据此在写 journal 之前拒绝提升。
fn dataset_member_mismatch() -> StorageError {
    StorageError::new(
        StorageErrorKind::CorruptTransaction(CorruptTransactionError::new(
            TransactionScope::Dataset,
            CorruptionReason::DatasetMemberDigestMismatch,
            QuarantineDisposition::EvidenceQuarantined,
        )),
        "上一代不完整或与持久化记录矛盾",
    )
}

fn manifest_record_from_journal(journal: &DatasetJournal) -> DatasetManifestRecord {
    DatasetManifestRecord {
        修订号: journal.新修订号.clone(),
        成员集合: journal
            .成员集合
            .iter()
            .map(|member| member.名称.clone())
            .collect(),
        成员证据: journal
            .成员集合
            .iter()
            .map(|member| DatasetManifestMemberRecord {
                名称: member.名称.clone(),
                字节数: member.字节数,
                修订摘要: member.修订摘要.clone(),
                内容摘要: member.摘要.clone(),
            })
            .collect(),
    }
}

fn manifest_from_record(
    dir: &Dir,
    generation_dir: &str,
    record: &DatasetManifestRecord,
) -> Result<DatasetManifest, StorageError> {
    let revision = DatasetRevision::from_bytes(proto::decode_revision(&record.修订号)?);
    let member_evidence = if record.成员证据.is_empty() && !record.成员集合.is_empty() {
        let blobs = PathBuf::from(generation_dir).join(BLOBS_DIR);
        record
            .成员集合
            .iter()
            .map(|name| {
                let safe_name = SafePathSegment::from_str(name)?;
                let bytes = proto::read_file(dir, &blobs.join(name))?.ok_or_else(|| {
                    StorageError::new(StorageErrorKind::Io, "数据集 manifest 引用的成员不存在")
                })?;
                Ok(DatasetMemberReference::from_manifest_member(
                    revision.clone(),
                    safe_name,
                    bytes.len() as u64,
                    revision_member_digest(&bytes),
                ))
            })
            .collect::<Result<Vec<_>, StorageError>>()?
    } else {
        record
            .成员证据
            .iter()
            .map(|member| {
                Ok(DatasetMemberReference::from_manifest_member(
                    revision.clone(),
                    SafePathSegment::from_str(&member.名称)?,
                    member.字节数,
                    proto::decode_revision(&member.修订摘要)?,
                ))
            })
            .collect::<Result<Vec<_>, StorageError>>()?
    };
    let manifest = DatasetManifest::from_verified_members(revision, member_evidence)?;
    let persisted_names = record
        .成员集合
        .iter()
        .map(|name| SafePathSegment::from_str(name))
        .collect::<Result<Vec<_>, _>>()?;
    if manifest.members() != persisted_names {
        return Err(StorageError::new(
            StorageErrorKind::Io,
            "数据集 manifest 成员集合与复用证据不一致",
        ));
    }
    Ok(manifest)
}

#[async_trait]
impl AtomicDatasetPort for FileSystemDatasetAdapter {
    async fn list_datasets(
        &self,
        namespace: crate::domain::StorageNamespace,
    ) -> Result<Vec<DatasetKey>, StorageError> {
        self.list_datasets_sync(namespace)
    }

    async fn delete_all_generations(
        &self,
        dataset: &DatasetKey,
        options: DeleteOptions,
    ) -> Result<DeleteOutcome, StorageError> {
        self.delete_all_generations_sync(dataset, options)
    }

    async fn read_manifest(&self, dataset: &DatasetKey) -> Result<DatasetManifest, StorageError> {
        let (dir, _lock) = self.locked(dataset)?;
        self.current_manifest(&dir)
    }

    async fn read_consistent(
        &self,
        dataset: &DatasetKey,
        members: &[SafePathSegment],
    ) -> Result<DatasetReadOutcome, StorageError> {
        let (dir, _lock) = self.locked(dataset)?;
        self.read_generation(&dir, PRIMARY_DIR, members)
    }

    async fn read_previous(
        &self,
        dataset: &DatasetKey,
        members: &[SafePathSegment],
    ) -> Result<DatasetReadOutcome, StorageError> {
        let (dir, _lock) = self.locked(dataset)?;
        self.read_generation(&dir, PREVIOUS_DIR, members)
    }

    async fn commit_atomic(
        &self,
        dataset: &DatasetKey,
        expected: &DatasetRevision,
        members: &[DatasetMember],
        _options: WriteOptions,
    ) -> Result<DatasetCommitReceipt, StorageError> {
        let (dir, _lock) = self.locked(dataset)?;
        self.commit_sync(&dir, expected, members)
    }

    async fn commit_incremental(
        &self,
        dataset: &DatasetKey,
        changes: &DatasetChangeSet,
        _options: WriteOptions,
    ) -> Result<DatasetCommitReceipt, StorageError> {
        let (dir, _lock) = self.locked(dataset)?;
        self.commit_incremental_sync(&dir, changes)
    }

    async fn promote_previous(
        &self,
        dataset: &DatasetKey,
    ) -> Result<DatasetCommitReceipt, StorageError> {
        let (dir, _lock) = self.locked(dataset)?;
        self.promote_sync(&dir)
    }

    async fn quarantine(
        &self,
        dataset: &DatasetKey,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcome, StorageError> {
        let (dir, _lock) = self.locked(dataset)?;
        self.quarantine_sync(&dir, generation, scope, reason)
    }
}

#[cfg(test)]
#[path = "dataset_filesystem_tests.rs"]
mod tests;
