use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{StorageError, StorageErrorKind};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) enum JournalPhase {
    Prepared,
    Committed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct BlobJournal {
    pub nonce: String,
    pub old_digest: Option<String>,
    pub new_digest: String,
    pub phase: JournalPhase,
}

pub(super) fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aemeath.storage.blob.bytes.v1\0");
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(super) fn digest_file(parent: &Dir, path: &Path) -> Result<Option<String>, StorageError> {
    let metadata = match parent.symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(map_io(error)),
    };
    if metadata.file_type().is_symlink() {
        return Err(StorageError::new(
            StorageErrorKind::InvalidKey,
            "事务协议文件是符号链接",
        ));
    }
    let mut file = parent.open(path).map_err(map_io)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(map_io)?;
    Ok(Some(digest(&bytes)))
}

pub(super) fn write_journal(
    parent: &Dir,
    primary_name: &Path,
    journal: &BlobJournal,
    sync: bool,
) -> Result<(), StorageError> {
    let journal_name = journal_name(primary_name);
    reject_symlink(parent, &journal_name)?;
    let stage_name = PathBuf::from(format!(".journal-{}", journal.nonce));
    let bytes = serde_json::to_vec(journal).map_err(|error| {
        StorageError::new(StorageErrorKind::Io, format!("事务日志编码失败：{error}"))
    })?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = parent.open_with(&stage_name, &options).map_err(map_io)?;
    file.write_all(&bytes).map_err(map_io)?;
    if sync {
        file.sync_all().map_err(map_sync)?;
    }
    drop(file);
    parent
        .rename(&stage_name, parent, &journal_name)
        .map_err(map_io)?;
    Ok(())
}

pub(super) fn read_journal(
    parent: &Dir,
    primary_name: &Path,
) -> Result<Option<BlobJournal>, StorageError> {
    let name = journal_name(primary_name);
    reject_symlink(parent, &name)?;
    let mut file = match parent.open(&name) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(map_io(error)),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(map_io)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| StorageError::new(StorageErrorKind::Io, "事务日志损坏且无法解码"))
}

pub(super) fn journal_name(primary_name: &Path) -> PathBuf {
    primary_name.with_extension("journal")
}

fn reject_symlink(parent: &Dir, path: &Path) -> Result<(), StorageError> {
    if let Ok(metadata) = parent.symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "事务协议文件是符号链接",
            ));
        }
    }
    Ok(())
}

fn map_io(error: std::io::Error) -> StorageError {
    let kind = if error.kind() == std::io::ErrorKind::PermissionDenied {
        StorageErrorKind::PermissionDenied
    } else {
        StorageErrorKind::Io
    };
    StorageError::new(kind, format!("事务 I/O 失败：{error}"))
}

fn map_sync(error: std::io::Error) -> StorageError {
    StorageError::new(
        StorageErrorKind::UnsupportedDurability,
        format!("当前平台无法兑现事务持久性：{error}"),
    )
}
