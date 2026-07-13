use std::{fs, io::Write};

use rustix::fs::{openat, renameat, statat, AtFlags, FileType, Mode, OFlags};
use uuid::Uuid;

use crate::contract::{StorageKey, StorageOperation, WriteReceipt};

use super::path::{map_errno, map_io, map_replace_errno, remove_stage, CapabilityRoot};

const MAX_STAGE_ATTEMPTS: usize = 16;

pub(super) trait StageNameSource: Send + Sync {
    fn next_name(&self) -> String;
}

pub(super) struct UuidStageNameSource;

impl StageNameSource for UuidStageNameSource {
    fn next_name(&self) -> String {
        format!(".aemeath-stage-{}", Uuid::new_v4())
    }
}

pub(super) trait CommitHooks: Send + Sync {
    fn after_open_parent(&self) -> std::io::Result<()> {
        Ok(())
    }

    fn before_replace(&self) -> std::io::Result<()>;

    fn replace(
        &self,
        parent: &fs::File,
        stage: &str,
        primary: &str,
    ) -> Result<(), rustix::io::Errno> {
        renameat(parent, stage, parent, primary)
    }

    fn after_replace(&self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) struct NoopCommitHooks;

impl CommitHooks for NoopCommitHooks {
    fn before_replace(&self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn write_atomic(
    root: &CapabilityRoot,
    key: &StorageKey,
    bytes: &[u8],
    stage_names: &dyn StageNameSource,
    hooks: &dyn CommitHooks,
) -> Result<WriteReceipt, crate::contract::StorageError> {
    let operation = StorageOperation::WriteAtomic;
    let (parent, primary) = root.open_parent(key, true, operation)?;
    hooks
        .after_open_parent()
        .map_err(|error| map_io(key, operation, error))?;
    ensure_safe_primary(&parent, &primary, key)?;
    let (stage, mut file) = create_stage(&parent, key, stage_names)?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| map_io(key, operation, error))?;
        drop(file);
        hooks
            .before_replace()
            .map_err(|error| map_io(key, operation, error))?;
        hooks
            .replace(&parent, stage.as_str(), primary.as_str())
            .map_err(|error| map_replace_errno(key, operation, error))?;
        hooks
            .after_replace()
            .map_err(|error| map_io(key, operation, error))?;
        Ok(WriteReceipt::committed())
    })();
    if result.is_err() {
        remove_stage(&parent, &stage);
    }
    result
}

fn ensure_safe_primary(
    parent: &fs::File,
    primary: &str,
    key: &StorageKey,
) -> Result<(), crate::contract::StorageError> {
    let stat = match statat(parent, primary, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(()),
        Err(error) => return Err(map_errno(key, StorageOperation::WriteAtomic, error)),
    };
    if FileType::from_raw_mode(stat.st_mode).is_file() {
        return Ok(());
    }
    Err(crate::contract::StorageError::new(
        crate::contract::StorageErrorKind::UnsafeFilesystemEntry,
        StorageOperation::WriteAtomic,
        key.clone(),
        std::io::Error::other("primary 不是普通文件"),
    ))
}

fn create_stage(
    parent: &fs::File,
    key: &StorageKey,
    stage_names: &dyn StageNameSource,
) -> Result<(String, fs::File), crate::contract::StorageError> {
    for _ in 0..MAX_STAGE_ATTEMPTS {
        let stage = stage_names.next_name();
        let flags =
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        match openat(parent, stage.as_str(), flags, Mode::RUSR | Mode::WUSR) {
            Ok(fd) => return Ok((stage, fs::File::from(fd))),
            Err(error) if error == rustix::io::Errno::EXIST => continue,
            Err(error) => return Err(map_errno(key, StorageOperation::WriteAtomic, error)),
        }
    }
    Err(map_io(
        key,
        StorageOperation::WriteAtomic,
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "随机 stage 名连续发生碰撞",
        ),
    ))
}
