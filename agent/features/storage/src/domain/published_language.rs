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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageNamespaceData {
    Session,
    Memory,
    ToolResult,
    Config,
    /// #252：后台进程账本（session 级快照，resume 失效对账与查询）。
    BackgroundProcess,
    /// #252：后台进程账本旧 namespace（Background Process 更名前的
    /// `background-task`），仅 resume 读旧兜底使用，新写入一律
    /// [`StorageNamespaceData::BackgroundProcess`]。
    LegacyBackgroundTask,
}

impl StorageNamespaceData {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Memory => "memory",
            Self::ToolResult => "tool-result",
            Self::Config => "config",
            Self::BackgroundProcess => "background-process",
            Self::LegacyBackgroundTask => "background-task",
        }
    }

    /// 当前所有在役 namespace 均要求 crash-safe（#1865 B：BestEffort 专属
    /// 分支随 AuditUsage 死变体删除）。
    pub fn minimum_durability(self) -> DurabilityData {
        DurabilityData::ProcessCrashSafe
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
    ) -> Result<Self, share::error::DomainError> {
        if segments.is_empty() {
            return Err(share::error::DomainError::from(StorageError::new(
                StorageErrorKind::InvalidKey,
                "存储键至少需要一个路径段",
            )));
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

    /// 逻辑 key → 存储根下的相对物理路径（`namespace/seg1/seg2`）。
    /// #1865 D：物理布局知识的唯一发布点——消费方（如 runtime 工具结果
    /// locator）NEVER 自行拼接 namespace/segment 顺序。
    pub fn relative_path(&self) -> std::path::PathBuf {
        self.segments.iter().fold(
            std::path::PathBuf::from(self.namespace.as_str()),
            |path, segment| path.join(segment.as_str()),
        )
    }
}

/// 语义映射（#1865 C）：InvalidKey→Invalid（调用方可修正）、UnsupportedDurability
/// →Unavailable（能力不可用）、其余→Storage（底层存储失败）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StorageErrorKind {
    InvalidKey,
    Io,
    PermissionDenied,
    UnsupportedDurability,
    ConcurrentWrite,
    CorruptTransaction(CorruptTransactionError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StorageError {
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

impl StorageError {
    /// 语义分类（DomainError 折叠的单一映射点）。
    pub(crate) fn category(&self) -> share::error::ErrorCategory {
        use share::error::ErrorCategory;
        match self.kind {
            StorageErrorKind::InvalidKey => ErrorCategory::Invalid,
            StorageErrorKind::UnsupportedDurability => ErrorCategory::Unavailable,
            StorageErrorKind::Io
            | StorageErrorKind::PermissionDenied
            | StorageErrorKind::ConcurrentWrite
            | StorageErrorKind::CorruptTransaction(_) => ErrorCategory::Storage,
        }
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for StorageError {}

/// 边界折叠（#1865 C）：storage 内部事实错误 → 全仓唯一跨界错误类型。
/// 消费方只按语义三档做控制流（NEVER 依赖 storage 的实现细节 kind）。
impl From<StorageError> for share::error::DomainError {
    fn from(error: StorageError) -> Self {
        share::error::DomainError::from_parts("storage", error.category(), error.message.clone())
            .with_source(std::sync::Arc::new(error))
    }
}
