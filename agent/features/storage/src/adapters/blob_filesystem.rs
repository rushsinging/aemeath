use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;

use async_trait::async_trait;
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use uuid::Uuid;

use crate::{
    AtomicBlobPort, BlobRead, DeleteOptions, DeleteOutcome, Durability, Generation, PreviousPolicy,
    PromoteOutcome, QuarantineOutcome, QuarantineReason, QuarantineReceipt, ReadOutcome,
    SafePathSegment, StorageError, StorageErrorKind, StorageKey, TransactionScope, WriteOptions,
    WriteReceipt,
};

#[derive(Debug)]
pub struct FileSystemBlobAdapter {
    root: Dir,
    promoted_keys: Mutex<HashSet<StorageKey>>,
}

impl FileSystemBlobAdapter {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        std::fs::create_dir_all(root.as_ref()).map_err(map_io)?;
        let root = Dir::open_ambient_dir(root.as_ref(), ambient_authority()).map_err(map_io)?;
        Ok(Self {
            root,
            promoted_keys: Mutex::new(HashSet::new()),
        })
    }

    fn relative_primary(key: &StorageKey) -> PathBuf {
        key.segments()
            .iter()
            .fold(PathBuf::from(key.namespace().as_str()), |path, segment| {
                path.join(segment.as_str())
            })
    }

    fn relative_generation(key: &StorageKey, generation: Generation) -> PathBuf {
        let primary = Self::relative_primary(key);
        match generation {
            Generation::Primary => primary,
            Generation::Previous => primary.with_extension("previous"),
        }
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

    fn write_sync(
        &self,
        key: &StorageKey,
        bytes: &[u8],
        options: WriteOptions,
    ) -> Result<WriteReceipt, StorageError> {
        let durability = key.namespace().effective_durability(options.durability());
        let (parent, primary_name) = self.prepare_parent(key)?;
        if let Ok(metadata) = parent.symlink_metadata(&primary_name) {
            if metadata.file_type().is_symlink() {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidKey,
                    "存储目标是符号链接",
                ));
            }
        }

        let stage_name = format!(".stage-{}", Uuid::new_v4());
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let mut stage = parent.open_with(&stage_name, &options).map_err(map_io)?;
            stage.write_all(bytes).map_err(map_io)?;
            if durability == Durability::ProcessCrashSafe {
                stage.sync_all().map_err(map_durability)?;
            }
            drop(stage);
            if key.namespace().previous_policy() == PreviousPolicy::Retain
                && parent.symlink_metadata(&primary_name).is_ok()
            {
                let previous_name = primary_name.with_extension("previous");
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
                    .rename(&primary_name, &parent, &previous_name)
                    .map_err(map_io)?;
            }
            parent
                .rename(&stage_name, &parent, &primary_name)
                .map_err(map_io)?;
            self.promoted_keys.lock().map_err(map_lock)?.remove(key);
            if durability == Durability::ProcessCrashSafe {
                let mut directory_options = OpenOptions::new();
                directory_options.read(true);
                parent
                    .open_with(".", &directory_options)
                    .and_then(|directory| directory.sync_all())
                    .map_err(map_durability)?;
            }
            Ok(WriteReceipt::committed(None))
        })();
        if result.is_err() {
            let _ = parent.remove_file(stage_name);
        }
        result
    }
}

#[async_trait]
impl AtomicBlobPort for FileSystemBlobAdapter {
    async fn read(
        &self,
        key: &StorageKey,
        generation: Generation,
    ) -> Result<ReadOutcome, StorageError> {
        let relative = Self::relative_generation(key, generation);
        let metadata = match self.root.symlink_metadata(&relative) {
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
        let mut file = self.root.open(&relative).map_err(map_io)?;
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
        let (parent, primary_name) = self.prepare_parent(key)?;
        let previous_name = primary_name.with_extension("previous");
        match parent.symlink_metadata(&previous_name) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let promoted = self.promoted_keys.lock().map_err(map_lock)?.contains(key);
                if promoted && parent.symlink_metadata(&primary_name).is_ok() {
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
        self.promoted_keys
            .lock()
            .map_err(map_lock)?
            .insert(key.clone());
        Ok(PromoteOutcome::Promoted(WriteReceipt::committed(None)))
    }

    async fn quarantine(
        &self,
        key: &StorageKey,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcome, StorageError> {
        let (parent, primary_name) = self.prepare_parent(key)?;
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
        let (parent, primary_name) = self.prepare_parent(key)?;
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
        self.promoted_keys.lock().map_err(map_lock)?.remove(key);
        Ok(DeleteOutcome::new(
            deleted_primary,
            deleted_previous,
            deleted_quarantine,
        ))
    }
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

fn map_lock<T>(error: std::sync::PoisonError<T>) -> StorageError {
    StorageError::new(
        StorageErrorKind::Io,
        format!("Storage 内部状态锁损坏：{error}"),
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
