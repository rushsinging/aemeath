use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};

use crate::{SafePathSegment, StorageError, StorageErrorKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafeStorageFileType {
    RegularFile,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafeStorageEntry {
    name: SafePathSegment,
    file_type: SafeStorageFileType,
}

impl SafeStorageEntry {
    pub fn name(&self) -> &SafePathSegment {
        &self.name
    }

    pub fn file_type(&self) -> SafeStorageFileType {
        self.file_type
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SafeOpenOptions {
    pub read: bool,
    pub append: bool,
}

#[derive(Clone)]
pub struct SafeStorageRoot {
    dir: Arc<Dir>,
}

impl SafeStorageRoot {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        std::fs::create_dir_all(root.as_ref()).map_err(map_io)?;
        let dir = Dir::open_ambient_dir(root.as_ref(), ambient_authority()).map_err(map_io)?;
        Ok(Self { dir: Arc::new(dir) })
    }

    pub fn ensure_dir(&self, segments: &[SafePathSegment]) -> Result<SafeStorageDir, StorageError> {
        let mut current = self.dir.try_clone().map_err(map_io)?;
        for segment in segments {
            reject_symlink(&current, Path::new(segment.as_str()))?;
            match current.create_dir(segment.as_str()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(map_io(error)),
            }
            reject_symlink(&current, Path::new(segment.as_str()))?;
            current = current.open_dir(segment.as_str()).map_err(map_io)?;
        }
        Ok(SafeStorageDir {
            dir: Arc::new(current),
        })
    }
}

#[derive(Clone)]
pub struct SafeStorageDir {
    dir: Arc<Dir>,
}

impl SafeStorageDir {
    pub fn open_existing(
        &self,
        name: &SafePathSegment,
        options: SafeOpenOptions,
    ) -> Result<std::fs::File, StorageError> {
        reject_regular_file_symlink(&self.dir, name)?;
        let options = cap_options(options, false);
        self.dir
            .open_with(name.as_str(), &options)
            .map(cap_std::fs::File::into_std)
            .map_err(map_io)
    }

    pub fn create_or_open(
        &self,
        name: &SafePathSegment,
        options: SafeOpenOptions,
    ) -> Result<std::fs::File, StorageError> {
        reject_regular_file_symlink(&self.dir, name)?;
        let options = cap_options(options, true);
        let file = self
            .dir
            .open_with(name.as_str(), &options)
            .map(cap_std::fs::File::into_std)
            .map_err(map_io)?;
        reject_regular_file_symlink(&self.dir, name)?;
        Ok(file)
    }

    pub fn entries(&self) -> Result<Vec<SafeStorageEntry>, StorageError> {
        let mut entries = Vec::new();
        for entry in self.dir.entries().map_err(map_io)? {
            let entry = entry.map_err(map_io)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(name) = SafePathSegment::from_str(&name) else {
                continue;
            };
            let metadata = entry.metadata().map_err(map_io)?;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                continue;
            }
            let file_type = if file_type.is_file() {
                SafeStorageFileType::RegularFile
            } else if file_type.is_dir() {
                SafeStorageFileType::Directory
            } else {
                continue;
            };
            entries.push(SafeStorageEntry { name, file_type });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }
}

fn cap_options(options: SafeOpenOptions, create: bool) -> OpenOptions {
    let mut result = OpenOptions::new();
    result
        .read(options.read)
        .append(options.append)
        .create(create);
    result
}

fn reject_regular_file_symlink(dir: &Dir, name: &SafePathSegment) -> Result<(), StorageError> {
    reject_symlink(dir, Path::new(name.as_str()))?;
    match dir.symlink_metadata(name.as_str()) {
        Ok(metadata) if !metadata.file_type().is_file() => Err(StorageError::new(
            StorageErrorKind::InvalidKey,
            "目标不是普通文件",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_io(error)),
    }
}

fn reject_symlink(dir: &Dir, path: &Path) -> Result<(), StorageError> {
    match dir.symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(StorageError::new(
            StorageErrorKind::InvalidKey,
            "路径目标是符号链接",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(map_io(error)),
    }
}

fn map_io(error: std::io::Error) -> StorageError {
    let kind = if error.kind() == std::io::ErrorKind::PermissionDenied {
        StorageErrorKind::PermissionDenied
    } else {
        StorageErrorKind::Io
    };
    StorageError::new(kind, "路径安全操作失败")
}
