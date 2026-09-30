use crate::domain::PreviousPolicy;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use async_trait::async_trait;
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use fs2::FileExt;
use uuid::Uuid;

use super::blob_protocol::{
    digest, digest_file, journal_name, read_journal, write_journal, BlobJournal, JournalPhase,
};
use crate::{
    AtomicBlobPort, BlobReadData, CorruptTransactionError, CorruptionReason, DeleteOptionsData,
    DeleteOutcomeData, DurabilityData, GenerationData, PromoteOutcomeData, QuarantineDisposition,
    QuarantineOutcomeData, QuarantineReason, QuarantineReceiptData, ReadOutcomeData,
    SafePathSegmentData, StorageEntryData, StorageError, StorageErrorKind, StorageKeyData,
    StorageNamespaceData, TransactionScopeData, WriteOptionsData, WriteReceiptData,
};

#[derive(Debug)]
enum FaultPoint {
    StageWrite,
    FileSync,
    UnsupportedDurability,
    PreviousNext,
    PreparedJournal,
    DirectorySync,
    AfterReplace,
    CommittedJournal,
    PreviousPromotion,
    Cleanup,
}

#[cfg(any(test, feature = "test-fault-injection"))]
const FAULT_POINT_ENV: &str = "AEMEATH_STORAGE_FAULT_POINT";
#[cfg(any(test, feature = "test-fault-injection"))]
const FAULT_ABORT_ENV: &str = "AEMEATH_STORAGE_FAULT_ABORT";

/// 故障注入配置：**adapter 实例状态**，不是进程全局状态。
///
/// 构造时快照一次（`from_env` 供崩溃恢复子进程从启动 env 接收父进程配置），
/// 此后运行期绝不读进程 env——从机制上消除跨测试 env 污染与 env 并发读写的数据竞争。
#[cfg(any(test, feature = "test-fault-injection"))]
#[derive(Clone, Debug, Default)]
pub(crate) struct BlobFaultInjector {
    requested: Option<String>,
    abort: bool,
}

#[cfg(any(test, feature = "test-fault-injection"))]
impl BlobFaultInjector {
    /// 请求在指定故障点注入（如 `cleanup`、`after_replace`）。
    /// 仅单元测试显式构造使用；feature 形态经 `from_env` 接收子进程配置。
    #[cfg(test)]
    pub(crate) fn requested(point: impl Into<String>) -> Self {
        Self {
            requested: Some(point.into()),
            abort: false,
        }
    }

    /// 构造时从启动 env 快照一次：子进程无法接收 Rust 对象，
    /// 崩溃恢复演练经父进程 spawn 时的 env 传递注入配置。
    pub(crate) fn from_env() -> Self {
        Self {
            requested: std::env::var(FAULT_POINT_ENV).ok(),
            abort: std::env::var_os(FAULT_ABORT_ENV).is_some(),
        }
    }
}

#[cfg(any(test, feature = "test-fault-injection"))]
fn fault_point_name(point: &FaultPoint) -> &'static str {
    match point {
        FaultPoint::StageWrite => "stage_write",
        FaultPoint::FileSync => "file_sync",
        FaultPoint::UnsupportedDurability => "unsupported_durability",
        FaultPoint::PreviousNext => "previous_next",
        FaultPoint::PreparedJournal => "prepared_journal",
        FaultPoint::DirectorySync => "directory_sync",
        FaultPoint::AfterReplace => "after_replace",
        FaultPoint::CommittedJournal => "committed_journal",
        FaultPoint::PreviousPromotion => "previous_promotion",
        FaultPoint::Cleanup => "cleanup",
    }
}

pub struct FileSystemBlobAdapter {
    root: Dir,
    #[cfg(any(test, feature = "test-fault-injection"))]
    faults: BlobFaultInjector,
}

impl FileSystemBlobAdapter {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        std::fs::create_dir_all(root.as_ref()).map_err(map_io)?;
        let root = Dir::open_ambient_dir(root.as_ref(), ambient_authority()).map_err(map_io)?;
        Ok(Self {
            root,
            #[cfg(any(test, feature = "test-fault-injection"))]
            faults: BlobFaultInjector::from_env(),
        })
    }

    /// 注入配置仅作用于本 adapter 实例（测试内显式切换故障点）。
    #[cfg(test)]
    pub(crate) fn set_faults(&mut self, faults: BlobFaultInjector) {
        self.faults = faults;
    }

    #[cfg(any(test, feature = "test-fault-injection"))]
    fn inject_fault(&self, point: FaultPoint) -> Result<(), StorageError> {
        let name = fault_point_name(&point);
        let requested_matches = self
            .faults
            .requested
            .as_deref()
            .is_some_and(|requested| requested == name);
        if !requested_matches {
            return Ok(());
        }
        if matches!(point, FaultPoint::UnsupportedDurability) {
            return Err(StorageError::new(
                StorageErrorKind::UnsupportedDurability,
                "injected unsupported durability capability",
            ));
        }
        if self.faults.abort {
            std::process::abort();
        }
        Err(StorageError::new(
            StorageErrorKind::Io,
            format!("injected storage fault: {name}"),
        ))
    }

    #[cfg(not(any(test, feature = "test-fault-injection")))]
    fn inject_fault(&self, _point: FaultPoint) -> Result<(), StorageError> {
        Ok(())
    }

    fn relative_primary(key: &StorageKeyData) -> PathBuf {
        key.segments()
            .iter()
            .fold(PathBuf::from(key.namespace().as_str()), |path, segment| {
                path.join(segment.as_str())
            })
    }

    fn prepare_parent(&self, key: &StorageKeyData) -> Result<(Dir, PathBuf), StorageError> {
        let primary = Self::relative_primary(key);
        let parent = primary
            .parent()
            .ok_or_else(|| StorageError::new(StorageErrorKind::InvalidKey, "存储键缺少父目录"))?;
        self.root.create_dir_all(parent).map_err(map_io)?;
        let parent_dir = self.root.open_dir(parent).map_err(map_io)?;
        let file_name = primary
            .file_name()
            .ok_or_else(|| StorageError::new(StorageErrorKind::InvalidKey, "存储键缺少文件名"))?;
        Ok((parent_dir, PathBuf::from(file_name)))
    }

    fn lock_key(&self, parent: &Dir, primary_name: &Path) -> Result<std::fs::File, StorageError> {
        let lock_name = primary_name.with_extension("lock");
        if let Ok(metadata) = parent.symlink_metadata(&lock_name) {
            if metadata.file_type().is_symlink() {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidKey,
                    "事务锁文件是符号链接",
                ));
            }
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        let lock = parent
            .open_with(&lock_name, &options)
            .map_err(map_io)?
            .into_std();
        #[cfg(feature = "test-fault-injection")]
        if let Some(marker) = std::env::var_os("AEMEATH_STORAGE_BLOB_LOCK_ATTEMPT") {
            std::fs::write(marker, b"attempt").map_err(map_io)?;
        }
        lock.lock_exclusive().map_err(map_lock_io)?;
        Ok(lock)
    }

    fn prepare_locked(
        &self,
        key: &StorageKeyData,
    ) -> Result<(Dir, PathBuf, std::fs::File), StorageError> {
        let (parent, primary_name) = self.prepare_parent(key)?;
        let lock = self.lock_key(&parent, &primary_name)?;
        self.recover_sync(&parent, &primary_name)?;
        Ok((parent, primary_name, lock))
    }

    fn is_protocol_artifact(name: &str) -> bool {
        name.starts_with(".stage-")
            || name.starts_with(".journal-")
            || name.ends_with(".previous")
            || name.ends_with(".previous.next")
            || name.ends_with(".journal")
            || name.ends_with(".lock")
            || name.ends_with(".promoted")
            || name.contains(".quarantine.")
    }

    fn list_primary_sync(
        &self,
        namespace: StorageNamespaceData,
    ) -> Result<Vec<StorageEntryData>, StorageError> {
        let namespace_path = Path::new(namespace.as_str());
        let namespace_dir = match self.root.open_dir(namespace_path) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(map_io(error)),
        };
        let mut entries = Vec::new();
        let mut prefix_segments = Vec::new();
        Self::collect_primary_entries(
            &namespace_dir,
            namespace,
            &mut prefix_segments,
            &mut entries,
        )?;
        entries.sort_by(|left, right| left.key().segments().cmp(right.key().segments()));
        Ok(entries)
    }

    /// 递归枚举 blob：子目录作为额外 key 段进入（session 按 project 分目录
    /// 布局依赖此行为）；协议工件与符号链接的排除规则与平铺一致。
    fn collect_primary_entries(
        directory: &Dir,
        namespace: StorageNamespaceData,
        prefix_segments: &mut Vec<SafePathSegmentData>,
        entries: &mut Vec<StorageEntryData>,
    ) -> Result<(), StorageError> {
        for entry in directory.entries().map_err(map_io)? {
            let entry = entry.map_err(map_io)?;
            let raw_name = entry.file_name().to_string_lossy().into_owned();
            if Self::is_protocol_artifact(&raw_name) {
                continue;
            }
            let segment = match SafePathSegmentData::from_str(&raw_name) {
                Ok(segment) => segment,
                Err(_) => continue,
            };
            let metadata = entry.metadata().map_err(map_io)?;
            if metadata.file_type().is_symlink() {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidKey,
                    "存储枚举遇到符号链接",
                ));
            }
            if metadata.file_type().is_file() {
                prefix_segments.push(segment);
                let key = StorageKeyData::new(namespace, prefix_segments.clone())?;
                prefix_segments.pop();
                entries.push(StorageEntryData::new(key, metadata.len() as usize));
            } else {
                let child = directory
                    .open_dir(Path::new(segment.as_str()))
                    .map_err(map_io)?;
                prefix_segments.push(segment);
                Self::collect_primary_entries(&child, namespace, prefix_segments, entries)?;
                prefix_segments.pop();
            }
        }
        Ok(())
    }

    fn recover_sync(&self, parent: &Dir, primary_name: &Path) -> Result<(), StorageError> {
        let Some(journal) = read_journal(parent, primary_name)? else {
            return self.recover_orphan_previous(parent, primary_name);
        };
        let observed = digest_file(parent, primary_name)?;
        match journal.phase {
            JournalPhase::Prepared if observed.as_deref() == Some(journal.new_digest.as_str()) => {
                let committed = BlobJournal {
                    phase: JournalPhase::Committed,
                    ..journal.clone()
                };
                write_journal(parent, primary_name, &committed, true)?;
            }
            JournalPhase::Prepared
                if observed == journal.old_digest
                    || (observed.is_none() && journal.old_digest.is_none()) =>
            {
                let _ = parent.remove_file(format!(".stage-{}", journal.nonce));
                let _ = parent.remove_file(primary_name.with_extension("previous.next"));
                parent
                    .remove_file(journal_name(primary_name))
                    .map_err(map_io)?;
                return Ok(());
            }
            JournalPhase::Committed if observed.as_deref() == Some(journal.new_digest.as_str()) => {
            }
            _ => {
                return Err(self.quarantine_corrupt_transaction(
                    parent,
                    primary_name,
                    &journal,
                    if journal.phase == JournalPhase::Committed {
                        CorruptionReason::CommittedDigestMismatch
                    } else {
                        CorruptionReason::PrimaryDigestMatchesNeitherGeneration
                    },
                ));
            }
        }
        let previous_next = primary_name.with_extension("previous.next");
        if parent.symlink_metadata(&previous_next).is_ok() {
            let previous = primary_name.with_extension("previous");
            let _ = parent.remove_file(&previous);
            parent
                .rename(&previous_next, parent, &previous)
                .map_err(map_io)?;
        }
        let _ = parent.remove_file(format!(".stage-{}", journal.nonce));
        parent
            .remove_file(journal_name(primary_name))
            .map_err(map_io)?;
        sync_directory(parent, DurabilityData::ProcessCrashSafe)
    }

    fn recover_orphan_previous(
        &self,
        parent: &Dir,
        primary_name: &Path,
    ) -> Result<(), StorageError> {
        let previous_next = primary_name.with_extension("previous.next");
        let Some(orphan_digest) = digest_file(parent, &previous_next)? else {
            return Ok(());
        };
        if digest_file(parent, primary_name)?.as_deref() == Some(orphan_digest.as_str()) {
            parent.remove_file(&previous_next).map_err(map_io)?;
            return sync_directory(parent, DurabilityData::ProcessCrashSafe);
        }
        let journal = BlobJournal {
            nonce: "orphan".to_string(),
            old_digest: None,
            new_digest: orphan_digest,
            phase: JournalPhase::Prepared,
        };
        Err(self.quarantine_corrupt_transaction(
            parent,
            primary_name,
            &journal,
            CorruptionReason::OrphanPreviousDigestMismatch,
        ))
    }

    fn quarantine_corrupt_transaction(
        &self,
        parent: &Dir,
        primary_name: &Path,
        journal: &BlobJournal,
        reason: CorruptionReason,
    ) -> StorageError {
        let id = Uuid::new_v4().simple().to_string();
        let candidates = [
            primary_name.to_path_buf(),
            primary_name.with_extension("previous.next"),
            journal_name(primary_name),
            PathBuf::from(format!(".stage-{}", journal.nonce)),
        ];
        let mut disposition = QuarantineDisposition::EvidenceQuarantined;
        for source in candidates {
            match parent.symlink_metadata(&source) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    disposition = QuarantineDisposition::QuarantineFailed;
                    continue;
                }
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    disposition = QuarantineDisposition::QuarantineFailed;
                    continue;
                }
                Ok(_) => {}
            }
            let label = source
                .file_name()
                .expect("protocol evidence always has a file name")
                .to_string_lossy();
            let target = PathBuf::from(format!("{label}.corrupt.{id}"));
            if parent.rename(&source, parent, target).is_err() {
                disposition = QuarantineDisposition::QuarantineFailed;
            }
        }
        let _ = sync_directory(parent, DurabilityData::ProcessCrashSafe);
        StorageError::new(
            StorageErrorKind::CorruptTransaction(CorruptTransactionError::new(
                TransactionScopeData::Blob,
                reason,
                disposition,
            )),
            "Storage 事务证据矛盾，已 fail-closed",
        )
    }

    fn write_sync(
        &self,
        key: &StorageKeyData,
        bytes: &[u8],
        options: WriteOptionsData,
    ) -> Result<WriteReceiptData, StorageError> {
        let durability = key.namespace().effective_durability(options.durability());
        let (parent, primary_name, _lock) = self.prepare_locked(key)?;
        if let Ok(metadata) = parent.symlink_metadata(&primary_name) {
            if metadata.file_type().is_symlink() {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidKey,
                    "存储目标是符号链接",
                ));
            }
        }

        let nonce = Uuid::new_v4().simple().to_string();
        let stage_name = format!(".stage-{nonce}");
        let mut crossed_commit = false;
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let mut stage = parent.open_with(&stage_name, &options).map_err(map_io)?;
            stage.write_all(bytes).map_err(map_io)?;
            self.inject_fault(FaultPoint::StageWrite)?;
            if durability == DurabilityData::ProcessCrashSafe {
                self.inject_fault(FaultPoint::UnsupportedDurability)?;
                stage.sync_all().map_err(map_durability)?;
                self.inject_fault(FaultPoint::FileSync)?;
            }
            drop(stage);
            let primary_exists = parent.symlink_metadata(&primary_name).is_ok();
            let old_digest = if primary_exists {
                Some(read_and_digest(&parent, &primary_name)?)
            } else {
                None
            };
            let journal = BlobJournal {
                nonce: nonce.clone(),
                old_digest,
                new_digest: digest(bytes),
                phase: JournalPhase::Prepared,
            };
            if key.namespace().previous_policy() == PreviousPolicy::Retain && primary_exists {
                let previous_name = primary_name.with_extension("previous");
                let previous_next_name = primary_name.with_extension("previous.next");
                if let Ok(metadata) = parent.symlink_metadata(&previous_next_name) {
                    if metadata.file_type().is_symlink() {
                        return Err(StorageError::new(
                            StorageErrorKind::InvalidKey,
                            "上一代事务目标是符号链接",
                        ));
                    }
                    parent.remove_file(&previous_next_name).map_err(map_io)?;
                }
                parent
                    .hard_link(&primary_name, &parent, &previous_next_name)
                    .map_err(map_io)?;
                self.inject_fault(FaultPoint::PreviousNext)?;
                if durability == DurabilityData::ProcessCrashSafe {
                    let previous_next = parent.open(&previous_next_name).map_err(map_io)?;
                    previous_next.sync_all().map_err(map_durability)?;
                }
                write_journal(
                    &parent,
                    &primary_name,
                    &journal,
                    durability == DurabilityData::ProcessCrashSafe,
                )?;
                self.inject_fault(FaultPoint::PreparedJournal)?;
                sync_directory(&parent, durability)?;
                self.inject_fault(FaultPoint::DirectorySync)?;
                parent
                    .rename(&stage_name, &parent, &primary_name)
                    .map_err(map_io)?;
                crossed_commit = true;
                self.inject_fault(FaultPoint::AfterReplace)?;
                if let Ok(metadata) = parent.symlink_metadata(&previous_name) {
                    if metadata.file_type().is_symlink() {
                        return Err(StorageError::new(
                            StorageErrorKind::InvalidKey,
                            "上一代存储目标是符号链接",
                        ));
                    }
                    parent.remove_file(&previous_name).map_err(map_io)?;
                }
                parent
                    .rename(&previous_next_name, &parent, &previous_name)
                    .map_err(map_io)?;
                self.inject_fault(FaultPoint::PreviousPromotion)?;
            } else {
                write_journal(
                    &parent,
                    &primary_name,
                    &journal,
                    durability == DurabilityData::ProcessCrashSafe,
                )?;
                self.inject_fault(FaultPoint::PreparedJournal)?;
                sync_directory(&parent, durability)?;
                self.inject_fault(FaultPoint::DirectorySync)?;
                parent
                    .rename(&stage_name, &parent, &primary_name)
                    .map_err(map_io)?;
                crossed_commit = true;
                self.inject_fault(FaultPoint::AfterReplace)?;
            }
            self.inject_fault(FaultPoint::CommittedJournal)?;
            let committed = BlobJournal {
                phase: JournalPhase::Committed,
                ..journal
            };
            write_journal(
                &parent,
                &primary_name,
                &committed,
                durability == DurabilityData::ProcessCrashSafe,
            )?;
            self.inject_fault(FaultPoint::CommittedJournal)?;
            let _ = parent.remove_file(promoted_marker_name(&primary_name));
            sync_directory(&parent, durability)?;
            parent
                .remove_file(journal_name(&primary_name))
                .map_err(map_io)?;
            self.inject_fault(FaultPoint::Cleanup)?;
            sync_directory(&parent, durability)?;
            Ok(WriteReceiptData::committed(None))
        })();
        match result {
            Ok(receipt) => Ok(receipt),
            Err(error) => {
                let _ = parent.remove_file(&stage_name);
                let committed = crossed_commit
                    || read_journal(&parent, &primary_name)
                        .ok()
                        .flatten()
                        .and_then(|journal| {
                            digest_file(&parent, &primary_name)
                                .ok()
                                .flatten()
                                .map(|observed| observed == journal.new_digest)
                        })
                        .unwrap_or(false);
                if committed {
                    let _ = self.recover_sync(&parent, &primary_name);
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "blob_write recovery_pending journal_cleanup_pending"
                    );
                    Ok(WriteReceiptData::committed(Some(
                        crate::CommitWarningData::JournalCleanupPending,
                    )))
                } else {
                    Err(error)
                }
            }
        }
    }
}

#[async_trait]
impl AtomicBlobPort for FileSystemBlobAdapter {
    async fn read(
        &self,
        key: &StorageKeyData,
        generation: GenerationData,
    ) -> Result<ReadOutcomeData, StorageError> {
        let (parent, primary_name, _lock) = self.prepare_locked(key)?;
        let relative = match generation {
            GenerationData::Primary => primary_name,
            GenerationData::Previous => primary_name.with_extension("previous"),
        };
        let metadata = match parent.symlink_metadata(&relative) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReadOutcomeData::NotFound);
            }
            Err(error) => return Err(map_io(error)),
        };
        if metadata.file_type().is_symlink() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "存储目标是符号链接",
            ));
        }
        let mut file = parent.open(&relative).map_err(map_io)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(map_io)?;
        Ok(ReadOutcomeData::Found(BlobReadData::new(generation, bytes)))
    }

    async fn write_atomic(
        &self,
        key: &StorageKeyData,
        bytes: &[u8],
        options: WriteOptionsData,
    ) -> Result<WriteReceiptData, StorageError> {
        self.write_sync(key, bytes, options)
    }

    async fn promote_previous(
        &self,
        key: &StorageKeyData,
    ) -> Result<PromoteOutcomeData, StorageError> {
        let (parent, primary_name, _lock) = self.prepare_locked(key)?;
        let previous_name = primary_name.with_extension("previous");
        match parent.symlink_metadata(&previous_name) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if parent
                    .symlink_metadata(promoted_marker_name(&primary_name))
                    .is_ok()
                    && parent.symlink_metadata(&primary_name).is_ok()
                {
                    return Ok(PromoteOutcomeData::AlreadyPromoted);
                }
                return Ok(PromoteOutcomeData::NotFound);
            }
            Err(error) => return Err(map_io(error)),
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidKey,
                    "上一代存储目标是符号链接",
                ));
            }
            Ok(_) => {}
        }

        if let Ok(metadata) = parent.symlink_metadata(&primary_name) {
            if metadata.file_type().is_symlink() {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidKey,
                    "主存储目标是符号链接",
                ));
            }
            let quarantine_name = quarantine_name(&primary_name, &Uuid::new_v4().to_string());
            parent
                .rename(&primary_name, &parent, quarantine_name)
                .map_err(map_io)?;
        }
        parent
            .rename(&previous_name, &parent, &primary_name)
            .map_err(map_io)?;
        write_promoted_marker(&parent, &primary_name)?;
        sync_directory(&parent, DurabilityData::ProcessCrashSafe)?;
        Ok(PromoteOutcomeData::Promoted(WriteReceiptData::committed(
            None,
        )))
    }

    async fn quarantine(
        &self,
        key: &StorageKeyData,
        generation: GenerationData,
        scope: TransactionScopeData,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcomeData, StorageError> {
        let (parent, primary_name, _lock) = self.prepare_locked(key)?;
        let source = match generation {
            GenerationData::Primary => primary_name.clone(),
            GenerationData::Previous => primary_name.with_extension("previous"),
        };
        match parent.symlink_metadata(&source) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(QuarantineOutcomeData::already_absent(
                    generation, scope, reason,
                ));
            }
            Err(error) => return Err(map_io(error)),
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidKey,
                    "隔离目标是符号链接",
                ));
            }
            Ok(_) => {}
        }

        let id = SafePathSegmentData::from_str(&Uuid::new_v4().simple().to_string())?;
        let target = quarantine_name(&primary_name, id.as_str());
        parent.rename(&source, &parent, target).map_err(map_io)?;
        Ok(QuarantineOutcomeData::Moved(QuarantineReceiptData::new(
            id, generation, scope, reason,
        )))
    }

    async fn delete_all_generations(
        &self,
        key: &StorageKeyData,
        options: DeleteOptionsData,
    ) -> Result<DeleteOutcomeData, StorageError> {
        let (parent, primary_name, _lock) = self.prepare_locked(key)?;
        let deleted_primary = remove_if_exists(&parent, &primary_name)?;
        let deleted_previous = remove_if_exists(&parent, &primary_name.with_extension("previous"))?;
        let mut deleted_quarantine = false;
        if options.include_quarantine() {
            let prefix = format!("{}.quarantine.", primary_name.to_string_lossy());
            for entry in parent.entries().map_err(map_io)? {
                let entry = entry.map_err(map_io)?;
                let name = entry.file_name();
                if name.to_string_lossy().starts_with(&prefix) {
                    parent.remove_file(&name).map_err(map_io)?;
                    deleted_quarantine = true;
                }
            }
        }
        let _ = parent.remove_file(promoted_marker_name(&primary_name));
        Ok(DeleteOutcomeData::new(
            deleted_primary,
            deleted_previous,
            deleted_quarantine,
        ))
    }

    async fn list_primary(
        &self,
        namespace: StorageNamespaceData,
    ) -> Result<Vec<StorageEntryData>, StorageError> {
        self.list_primary_sync(namespace)
    }
}

fn promoted_marker_name(primary: &Path) -> PathBuf {
    primary.with_extension("promoted")
}

fn write_promoted_marker(parent: &Dir, primary: &Path) -> Result<(), StorageError> {
    let marker = promoted_marker_name(primary);
    if let Ok(metadata) = parent.symlink_metadata(&marker) {
        if metadata.file_type().is_symlink() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "promote marker 是符号链接",
            ));
        }
        parent.remove_file(&marker).map_err(map_io)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = parent.open_with(&marker, &options).map_err(map_io)?;
    file.write_all(b"promoted-v1").map_err(map_io)?;
    file.sync_all().map_err(map_durability)
}

fn read_and_digest(parent: &Dir, path: &Path) -> Result<String, StorageError> {
    let mut file = parent.open(path).map_err(map_io)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(map_io)?;
    Ok(digest(&bytes))
}

fn sync_directory(parent: &Dir, durability: DurabilityData) -> Result<(), StorageError> {
    if durability == DurabilityData::ProcessCrashSafe {
        let mut directory_options = OpenOptions::new();
        directory_options.read(true);
        parent
            .open_with(".", &directory_options)
            .and_then(|directory| directory.sync_all())
            .map_err(map_durability)?;
    }
    Ok(())
}

fn quarantine_name(primary: &Path, id: &str) -> PathBuf {
    let name = primary
        .file_name()
        .expect("validated StorageKeyData always has a file name")
        .to_string_lossy();
    PathBuf::from(format!("{name}.quarantine.{id}"))
}

fn remove_if_exists(parent: &Dir, path: &Path) -> Result<bool, StorageError> {
    match parent.symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(map_io(error)),
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StorageError::new(
            StorageErrorKind::InvalidKey,
            "删除目标是符号链接",
        )),
        Ok(_) => {
            parent.remove_file(path).map_err(map_io)?;
            Ok(true)
        }
    }
}

fn map_lock_io(error: std::io::Error) -> StorageError {
    StorageError::new(
        StorageErrorKind::ConcurrentWrite,
        format!("Storage key 锁获取失败：{error}"),
    )
}

fn map_io(error: std::io::Error) -> StorageError {
    let kind = if error.kind() == std::io::ErrorKind::PermissionDenied {
        StorageErrorKind::PermissionDenied
    } else {
        StorageErrorKind::Io
    };
    StorageError::new(kind, format!("存储 I/O 失败：{error}"))
}

fn map_durability(error: std::io::Error) -> StorageError {
    StorageError::new(
        StorageErrorKind::UnsupportedDurability,
        format!("当前平台无法兑现持久性要求：{error}"),
    )
}

// ---------------------------------------------------------------------------
// #[cfg(test)] blob RecoveryPending 终态日志 TDD —— 外置到
// blob_filesystem_tests.rs，通过 `#[path]` 引入。
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "blob_filesystem_tests.rs"]
mod tests;
