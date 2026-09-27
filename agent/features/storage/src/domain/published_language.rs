use std::error::Error;
use std::fmt;

use super::{CorruptTransactionError, SafePathSegmentData};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurabilityData {
    BestEffort,
    ProcessCrashSafe,
}

impl DurabilityData {
    pub fn satisfies(self, required: Self) -> bool {
        matches!(
            (self, required),
            (Self::ProcessCrashSafe, _) | (Self::BestEffort, Self::BestEffort)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreviousPolicy {
    Retain,
    Discard,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageNamespaceData {
    Session,
    Memory,
    TaskData,
    History,
    ToolResult,
    AuditUsage,
    Config,
    Workspace,
}

impl StorageNamespaceData {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Memory => "memory",
            Self::TaskData => "task",
            Self::History => "history",
            Self::ToolResult => "tool-result",
            Self::AuditUsage => "audit-usage",
            Self::Config => "config",
            Self::Workspace => "workspace",
        }
    }

    pub(crate) fn previous_policy(self) -> PreviousPolicy {
        match self {
            Self::AuditUsage => PreviousPolicy::Discard,
            Self::Session
            | Self::Memory
            | Self::TaskData
            | Self::History
            | Self::ToolResult
            | Self::Config
            | Self::Workspace => PreviousPolicy::Retain,
        }
    }

    pub fn minimum_durability(self) -> DurabilityData {
        match self {
            Self::AuditUsage => DurabilityData::BestEffort,
            Self::Session
            | Self::Memory
            | Self::TaskData
            | Self::History
            | Self::ToolResult
            | Self::Config
            | Self::Workspace => DurabilityData::ProcessCrashSafe,
        }
    }

    pub fn effective_durability(self, requested: DurabilityData) -> DurabilityData {
        if requested.satisfies(self.minimum_durability()) {
            requested
        } else {
            self.minimum_durability()
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StorageKeyData {
    namespace: StorageNamespaceData,
    segments: Vec<SafePathSegmentData>,
}

impl StorageKeyData {
    pub fn new(
        namespace: StorageNamespaceData,
        segments: Vec<SafePathSegmentData>,
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

    pub fn namespace(&self) -> StorageNamespaceData {
        self.namespace
    }

    pub fn segments(&self) -> &[SafePathSegmentData] {
        &self.segments
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageErrorKind {
    InvalidKey,
    Io,
    PermissionDenied,
    UnsupportedDurability,
    ConcurrentWrite,
    CorruptTransaction(CorruptTransactionError),
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

    pub fn kind(&self) -> &StorageErrorKind {
        &self.kind
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for StorageError {}
