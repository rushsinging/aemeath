use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use cap_std::io_lifetimes::AsFilelike;
use uuid::Uuid;

use crate::{
    AtomicBlobPort, BlobRead, Durability, Generation, ReadOutcome, StorageError, StorageErrorKind,
    StorageKey, WriteOptions, WriteReceipt,
};

#[derive(Debug)]
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
            parent
                .rename(&stage_name, &parent, &primary_name)
                .map_err(map_io)?;
            if durability == Durability::ProcessCrashSafe {
                parent
                    .as_filelike_view::<std::fs::File>()
                    .sync_all()
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
