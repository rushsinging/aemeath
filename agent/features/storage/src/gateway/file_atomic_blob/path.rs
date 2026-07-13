use std::{fs, io, path::Path};

use rustix::fs::{fstat, openat, AtFlags, FileType, Mode, OFlags, CWD};

use crate::contract::{
    BlobRead, ReadOutcome, StorageError, StorageErrorKind, StorageKey, StorageOperation,
};

pub(super) struct CapabilityRoot {
    directory: fs::File,
}

impl CapabilityRoot {
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        fs::create_dir_all(path)?;
        let fd = openat(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        Ok(Self {
            directory: fs::File::from(fd),
        })
    }

    pub(super) fn read(&self, key: &StorageKey) -> Result<ReadOutcome, StorageError> {
        let (parent, file_name) = match self.open_parent(key, false, StorageOperation::Read) {
            Ok(value) => value,
            Err(error) if error.is_not_found() => return Ok(ReadOutcome::NotFound),
            Err(error) => return Err(error),
        };
        let fd = match openat(
            &parent,
            file_name,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => fd,
            Err(error) if error == rustix::io::Errno::NOENT => return Ok(ReadOutcome::NotFound),
            Err(error) => return Err(map_errno(key, StorageOperation::Read, error)),
        };
        let stat = fstat(&fd).map_err(|error| map_errno(key, StorageOperation::Read, error))?;
        if !FileType::from_raw_mode(stat.st_mode).is_file() {
            return Err(StorageError::new(
                StorageErrorKind::UnsafeFilesystemEntry,
                StorageOperation::Read,
                key.clone(),
                io::Error::other("primary 不是普通文件"),
            ));
        }
        let mut file = fs::File::from(fd);
        let mut bytes = Vec::new();
        use std::io::Read;
        file.read_to_end(&mut bytes)
            .map_err(|error| map_io(key, StorageOperation::Read, error))?;
        Ok(ReadOutcome::Found(BlobRead::new(bytes)))
    }

    pub(super) fn open_parent(
        &self,
        key: &StorageKey,
        create: bool,
        operation: StorageOperation,
    ) -> Result<(fs::File, String), StorageError> {
        let mut current = self
            .directory
            .try_clone()
            .map_err(|error| map_io(key, operation, error))?;
        let namespace = key.namespace().as_str();
        current = open_or_create_dir(&current, namespace, create, key, operation)?;
        let (file_name, parents) = key
            .segments()
            .split_last()
            .expect("StorageKey 构造保证至少一个路径段");
        for segment in parents {
            current = open_or_create_dir(&current, segment.as_str(), create, key, operation)?;
        }
        Ok((current, file_name.as_str().to_owned()))
    }
}

fn open_or_create_dir(
    parent: &fs::File,
    name: &str,
    create: bool,
    key: &StorageKey,
    operation: StorageOperation,
) -> Result<fs::File, StorageError> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match openat(parent, name, flags, Mode::empty()) {
        Ok(fd) => Ok(fs::File::from(fd)),
        Err(error) if create && error == rustix::io::Errno::NOENT => {
            match rustix::fs::mkdirat(parent, name, Mode::RWXU) {
                Ok(()) => {}
                Err(error) if error == rustix::io::Errno::EXIST => {}
                Err(error) => return Err(map_errno(key, operation, error)),
            }
            openat(parent, name, flags, Mode::empty())
                .map(fs::File::from)
                .map_err(|error| map_errno(key, operation, error))
        }
        Err(error) => Err(map_errno(key, operation, error)),
    }
}

pub(super) fn map_errno(
    key: &StorageKey,
    operation: StorageOperation,
    error: rustix::io::Errno,
) -> StorageError {
    map_io(key, operation, io::Error::from(error))
}

pub(super) fn map_replace_errno(
    key: &StorageKey,
    operation: StorageOperation,
    error: rustix::io::Errno,
) -> StorageError {
    let io_error = io::Error::from(error);
    if error == rustix::io::Errno::XDEV || error == rustix::io::Errno::NOSYS {
        return StorageError::new(
            StorageErrorKind::UnsupportedAtomicReplace,
            operation,
            key.clone(),
            io_error,
        );
    }
    map_io(key, operation, io_error)
}

pub(super) fn map_io(
    key: &StorageKey,
    operation: StorageOperation,
    error: io::Error,
) -> StorageError {
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => StorageErrorKind::PermissionDenied,
        _ if matches!(
            error.raw_os_error(),
            Some(code)
                if code == rustix::io::Errno::LOOP.raw_os_error()
                    || code == rustix::io::Errno::NOTDIR.raw_os_error()
        ) =>
        {
            StorageErrorKind::UnsafeFilesystemEntry
        }
        _ => StorageErrorKind::Io,
    };
    StorageError::new(kind, operation, key.clone(), error)
}

pub(super) fn join_error(
    key: &StorageKey,
    operation: StorageOperation,
    error: tokio::task::JoinError,
) -> StorageError {
    StorageError::new(StorageErrorKind::Io, operation, key.clone(), error)
}

pub(super) fn remove_stage(parent: &fs::File, stage: &str) {
    let _ = rustix::fs::unlinkat(parent, stage, AtFlags::empty());
}
