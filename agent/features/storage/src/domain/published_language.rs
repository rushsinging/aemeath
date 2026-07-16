use std::error::Error;
use std::fmt;

use super::SafePathSegment;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Durability {
    BestEffort,
    ProcessCrashSafe,
}

impl Durability {
    pub fn satisfies(self, required: Self) -> bool {
        matches!(
            (self, required),
            (Self::ProcessCrashSafe, _) | (Self::BestEffort, Self::BestEffort)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageNamespace {
    Session,
    Memory,
    Task,
    History,
    ToolResult,
    AuditUsage,
    Config,
    Workspace,
    Cost,
}

impl StorageNamespace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Memory => "memory",
            Self::Task => "task",
            Self::History => "history",
            Self::ToolResult => "tool-result",
            Self::AuditUsage => "audit-usage",
            Self::Config => "config",
            Self::Workspace => "workspace",
            Self::Cost => "cost",
        }
    }

    pub fn minimum_durability(self) -> Durability {
        match self {
            Self::AuditUsage => Durability::BestEffort,
            Self::Session
            | Self::Memory
            | Self::Task
            | Self::History
            | Self::ToolResult
            | Self::Config
            | Self::Workspace
            | Self::Cost => Durability::ProcessCrashSafe,
        }
    }

    pub fn effective_durability(self, requested: Durability) -> Durability {
        if requested.satisfies(self.minimum_durability()) {
            requested
        } else {
            self.minimum_durability()
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StorageKey {
    namespace: StorageNamespace,
    segments: Vec<SafePathSegment>,
}

impl StorageKey {
    pub fn new(
        namespace: StorageNamespace,
        segments: Vec<SafePathSegment>,
    ) -> Result<Self, StorageError> {
        if segments.is_empty() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidKey,
                "存储键至少需要一个路径段",
            ));
        }
        Ok(Self {
            namespace,
            segments,
        })
    }

    pub fn namespace(&self) -> StorageNamespace {
        self.namespace
    }

    pub fn segments(&self) -> &[SafePathSegment] {
        &self.segments
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageErrorKind {
    InvalidKey,
    Io,
    PermissionDenied,
    UnsupportedDurability,
    ConcurrentWrite,
    CorruptTransaction,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageError {
    kind: StorageErrorKind,
    message: String,
}

impl StorageError {
    pub fn new(kind: StorageErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> StorageErrorKind {
        self.kind
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for StorageError {}
