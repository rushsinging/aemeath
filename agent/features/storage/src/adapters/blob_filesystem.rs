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
    AtomicBlobPort, BlobRead, CorruptTransactionError, CorruptionReason, DeleteOptions,
    DeleteOutcome, Durability, Generation, PreviousPolicy, PromoteOutcome, QuarantineDisposition,
    QuarantineOutcome, QuarantineReason, QuarantineReceipt, ReadOutcome, SafePathSegment,
    StorageEntry, StorageError, StorageErrorKind, StorageKey, StorageNamespace, TransactionScope,
    WriteOptions, WriteReceipt,
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
fn inject_fault(point: FaultPoint) -> Result<(), StorageError> {
    let requested = std::env::var_os("AEMEATH_STORAGE_FAULT_POINT");
    let name = match point {
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
    };
    if requested.as_deref() != Some(std::ffi::OsStr::new(name)) {
        return Ok(());
    }
    if matches!(point, FaultPoint::UnsupportedDurability) {
        return Err(StorageError::new(
            StorageErrorKind::UnsupportedDurability,
            "injected unsupported durability capability",
        ));
    }
    if std::env::var_os("AEMEATH_STORAGE_FAULT_ABORT").is_some() {
        std::process::abort();
    }
    Err(StorageError::new(
        StorageErrorKind::Io,
        format!("injected storage fault: {name}"),
    ))
}

#[cfg(not(any(test, feature = "test-fault-injection")))]
fn inject_fault(_point: FaultPoint) -> Result<(), StorageError> {
    Ok(())
}

pub struct FileSystemBlobAdapter {
    root: Dir,
}

impl FileSystemBlobAdapter {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        std::fs::create_dir_all(root.as_ref()).map_err(map_io)?;
        let root = Dir::open_ambient_dir(root.as_ref(), ambient_authority()).map_err(map_io)?;
        Ok(Self { root })
    }

    fn relative_primary(key: &StorageKey) -> PathBuf {
        key.segments()
            .iter()
            .fold(PathBuf::from(key.namespace().as_str()), |path, segment| {
                path.join(segment.as_str())
            })
    }

    fn prepare_parent(&self, key: &StorageKey) -> Result<(Dir, PathBuf), StorageError> {
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
        key: &StorageKey,
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
        namespace: StorageNamespace,
    ) -> Result<Vec<StorageEntry>, StorageError> {
        let namespace_path = Path::new(namespace.as_str());
        let namespace_dir = match self.root.open_dir(namespace_path) {
            Ok(directory) => directory,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(map_io(error)),
        };
        let mut entries = Vec::new();
        for entry in namespace_dir.entries().map_err(map_io)? {
            let entry = entry.map_err(map_io)?;
            let raw_name = entry.file_name().to_string_lossy().into_owned();
            if Self::is_protocol_artifact(&raw_name) {
                continue;
            }
            let segment = match SafePathSegment::from_str(&raw_name) {
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
            if !metadata.file_type().is_file() {
                continue;
            }
            let key = StorageKey::new(namespace, vec![segment])?;
            entries.push(StorageEntry::new(key, metadata.len() as usize));
        }
        entries.sort_by(|left, right| left.key().segments().cmp(right.key().segments()));
        Ok(entries)
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
        sync_directory(parent, Durability::ProcessCrashSafe)
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
            return sync_directory(parent, Durability::ProcessCrashSafe);
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
        let _ = sync_directory(parent, Durability::ProcessCrashSafe);
        StorageError::new(
            StorageErrorKind::CorruptTransaction(CorruptTransactionError::new(
                TransactionScope::Blob,
                reason,
                disposition,
            )),
            "Storage 事务证据矛盾，已 fail-closed",
        )
    }

    fn write_sync(
        &self,
        key: &StorageKey,
        bytes: &[u8],
        options: WriteOptions,
    ) -> Result<WriteReceipt, StorageError> {
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
            inject_fault(FaultPoint::StageWrite)?;
            if durability == Durability::ProcessCrashSafe {
                inject_fault(FaultPoint::UnsupportedDurability)?;
                stage.sync_all().map_err(map_durability)?;
                inject_fault(FaultPoint::FileSync)?;
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
                inject_fault(FaultPoint::PreviousNext)?;
                if durability == Durability::ProcessCrashSafe {
                    let previous_next = parent.open(&previous_next_name).map_err(map_io)?;
                    previous_next.sync_all().map_err(map_durability)?;
                }
                write_journal(
                    &parent,
                    &primary_name,
                    &journal,
                    durability == Durability::ProcessCrashSafe,
                )?;
                inject_fault(FaultPoint::PreparedJournal)?;
                sync_directory(&parent, durability)?;
                inject_fault(FaultPoint::DirectorySync)?;
                parent
                    .rename(&stage_name, &parent, &primary_name)
                    .map_err(map_io)?;
                crossed_commit = true;
                inject_fault(FaultPoint::AfterReplace)?;
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
                inject_fault(FaultPoint::PreviousPromotion)?;
            } else {
                write_journal(
                    &parent,
                    &primary_name,
                    &journal,
                    durability == Durability::ProcessCrashSafe,
                )?;
                inject_fault(FaultPoint::PreparedJournal)?;
                sync_directory(&parent, durability)?;
                inject_fault(FaultPoint::DirectorySync)?;
                parent
                    .rename(&stage_name, &parent, &primary_name)
                    .map_err(map_io)?;
                crossed_commit = true;
                inject_fault(FaultPoint::AfterReplace)?;
            }
            inject_fault(FaultPoint::CommittedJournal)?;
            let committed = BlobJournal {
                phase: JournalPhase::Committed,
                ..journal
            };
            write_journal(
                &parent,
                &primary_name,
                &committed,
                durability == Durability::ProcessCrashSafe,
            )?;
            inject_fault(FaultPoint::CommittedJournal)?;
            let _ = parent.remove_file(promoted_marker_name(&primary_name));
            sync_directory(&parent, durability)?;
            parent
                .remove_file(journal_name(&primary_name))
                .map_err(map_io)?;
            inject_fault(FaultPoint::Cleanup)?;
            sync_directory(&parent, durability)?;
            Ok(WriteReceipt::committed(None))
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
                    Ok(WriteReceipt::committed(Some(
                        crate::CommitWarning::JournalCleanupPending,
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
        key: &StorageKey,
        generation: Generation,
    ) -> Result<ReadOutcome, StorageError> {
        let (parent, primary_name, _lock) = self.prepare_locked(key)?;
        let relative = match generation {
            Generation::Primary => primary_name,
            Generation::Previous => primary_name.with_extension("previous"),
        };
        let metadata = match parent.symlink_metadata(&relative) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReadOutcome::NotFound);
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
        Ok(ReadOutcome::Found(BlobRead::new(generation, bytes)))
    }

    async fn write_atomic(
        &self,
        key: &StorageKey,
        bytes: &[u8],
        options: WriteOptions,
    ) -> Result<WriteReceipt, StorageError> {
        self.write_sync(key, bytes, options)
    }

    async fn promote_previous(&self, key: &StorageKey) -> Result<PromoteOutcome, StorageError> {
        let (parent, primary_name, _lock) = self.prepare_locked(key)?;
        let previous_name = primary_name.with_extension("previous");
        match parent.symlink_metadata(&previous_name) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if parent
                    .symlink_metadata(promoted_marker_name(&primary_name))
                    .is_ok()
                    && parent.symlink_metadata(&primary_name).is_ok()
                {
                    return Ok(PromoteOutcome::AlreadyPromoted);
                }
                return Ok(PromoteOutcome::NotFound);
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
        sync_directory(&parent, Durability::ProcessCrashSafe)?;
        Ok(PromoteOutcome::Promoted(WriteReceipt::committed(None)))
    }

    async fn quarantine(
        &self,
        key: &StorageKey,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcome, StorageError> {
        let (parent, primary_name, _lock) = self.prepare_locked(key)?;
        let source = match generation {
            Generation::Primary => primary_name.clone(),
            Generation::Previous => primary_name.with_extension("previous"),
        };
        match parent.symlink_metadata(&source) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(QuarantineOutcome::already_absent(generation, scope, reason));
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

        let id = SafePathSegment::from_str(&Uuid::new_v4().simple().to_string())?;
        let target = quarantine_name(&primary_name, id.as_str());
        parent.rename(&source, &parent, target).map_err(map_io)?;
        Ok(QuarantineOutcome::Moved(QuarantineReceipt::new(
            id, generation, scope, reason,
        )))
    }

    async fn delete_all_generations(
        &self,
        key: &StorageKey,
        options: DeleteOptions,
    ) -> Result<DeleteOutcome, StorageError> {
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
        Ok(DeleteOutcome::new(
            deleted_primary,
            deleted_previous,
            deleted_quarantine,
        ))
    }

    async fn list_primary(
        &self,
        namespace: StorageNamespace,
    ) -> Result<Vec<StorageEntry>, StorageError> {
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

fn sync_directory(parent: &Dir, durability: Durability) -> Result<(), StorageError> {
    if durability == Durability::ProcessCrashSafe {
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
        .expect("validated StorageKey always has a file name")
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
