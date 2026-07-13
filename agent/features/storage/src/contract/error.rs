use std::{error::Error as StdError, fmt};

use super::StorageKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StorageErrorKind {
    Io,
    PermissionDenied,
    UnsupportedAtomicReplace,
    UnsafeFilesystemEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageOperation {
    Read,
    WriteAtomic,
}

pub struct StorageError {
    kind: StorageErrorKind,
    operation: StorageOperation,
    key: StorageKey,
    source: Box<dyn StdError + Send + Sync>,
}

impl StorageError {
    pub fn new(
        kind: StorageErrorKind,
        operation: StorageOperation,
        key: StorageKey,
        source: impl StdError + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            operation,
            key,
            source: Box::new(source),
        }
    }

    pub const fn kind(&self) -> StorageErrorKind {
        self.kind
    }

    pub const fn operation(&self) -> StorageOperation {
        self.operation
    }

    pub const fn key(&self) -> &StorageKey {
        &self.key
    }

    pub(crate) fn is_not_found(&self) -> bool {
        self.source
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }
}

impl fmt::Debug for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StorageError")
            .field("kind", &self.kind)
            .field("operation", &self.operation)
            .field("key", &self.key)
            .field("source", &self.source.to_string())
            .finish()
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "存储操作 {:?} 失败（{}，类型 {:?}）：{}",
            self.operation, self.key, self.kind, self.source
        )
    }
}

impl StdError for StorageError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(self.source.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, io};

    use super::*;
    use crate::contract::{SafePathSegment, StorageNamespace};

    #[test]
    fn test_storage_error_preserves_public_context() {
        let key = StorageKey::new(
            StorageNamespace::Memory,
            [SafePathSegment::new("project.json").unwrap()],
        )
        .unwrap();
        let error = StorageError::new(
            StorageErrorKind::PermissionDenied,
            StorageOperation::WriteAtomic,
            key.clone(),
            io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
        );

        assert_eq!(error.kind(), StorageErrorKind::PermissionDenied);
        assert_eq!(error.operation(), StorageOperation::WriteAtomic);
        assert_eq!(error.key(), &key);
        assert_eq!(error.source().unwrap().to_string(), "denied");
        assert!(error.to_string().contains("存储操作"));
        assert!(error.to_string().contains("memory/project.json"));
    }

    #[test]
    fn test_storage_error_debug_does_not_contain_physical_root() {
        let key = StorageKey::new(
            StorageNamespace::Audit,
            [SafePathSegment::new("events").unwrap()],
        )
        .unwrap();
        let error = StorageError::new(
            StorageErrorKind::Io,
            StorageOperation::Read,
            key,
            io::Error::other("read failed"),
        );
        assert!(!format!("{error:?}").contains("/Users/"));
    }
}
