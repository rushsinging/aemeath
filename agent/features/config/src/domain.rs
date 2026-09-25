//! 领域层：config 的值对象、领域事件、错误分类与 port 间传递的预备状态。
//!
//! R8 方向：domain NEVER 依赖 application / ports / adapters。
use share::config::domain::merge::ConfigPatch;
use share::config::domain::scope::ConfigApplicationScope;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::{Config, MemoryConfig, PermissionModeConfig};
use std::path::{Path, PathBuf};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFieldData {
    Model,
    PermissionMode,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigChangeCauseData {
    ClientUpdate,
    ProjectCommit,
    FileReload,
}

#[derive(Debug, Clone)]
pub struct ConfigChangeData {
    pub cause: ConfigChangeCauseData,
    pub fields: Vec<ConfigFieldData>,
    pub snapshot: ConfigSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigRefreshError {
    Io,
    Parse,
    Invalid,
}

#[derive(Debug, Clone)]
pub enum ConfigRefreshOutcomeData {
    Unchanged,
    Reloaded {
        snapshot: ConfigSnapshot,
        scopes: Vec<ConfigApplicationScope>,
    },
    Rejected {
        error: ConfigRefreshError,
    },
}

#[derive(Debug)]
pub struct ConfigSubscriptionData {
    pub initial: ConfigSnapshot,
    pub changes: watch::Receiver<ConfigSnapshot>,
}

#[derive(Debug, Clone)]
pub enum ConfigUpdateData {
    SetModel { model: String },
    SetPermissionMode { mode: PermissionModeConfig },
    SetMemoryConfig { config: MemoryConfig },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigUpdateError {
    Invalid(String),
    Persist(ConfigPersistError),
}

impl std::fmt::Display for ConfigUpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => write!(formatter, "配置更新非法：{message}"),
            Self::Persist(error) => write!(formatter, "配置持久化失败：{error}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectConfigLocationData {
    canonical_search_root: PathBuf,
    key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectConfigLocationError {
    NotAbsolute,
    NotCanonical,
    EmptyIdentity,
}

impl std::fmt::Display for ProjectConfigLocationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAbsolute => write!(formatter, "项目配置搜索根路径必须是绝对路径"),
            Self::NotCanonical => write!(formatter, "项目配置搜索根路径必须是规范化路径"),
            Self::EmptyIdentity => write!(formatter, "项目配置身份不能为空"),
        }
    }
}

impl std::error::Error for ProjectConfigLocationError {}

impl ProjectConfigLocationData {
    pub fn try_from_project_identity(
        canonical_search_root: PathBuf,
        stable_identity: &[u8],
    ) -> Result<Self, share::error::DomainError> {
        if !canonical_search_root.is_absolute() {
            return Err(ProjectConfigLocationError::NotAbsolute.into());
        }
        if stable_identity.is_empty() {
            return Err(ProjectConfigLocationError::EmptyIdentity.into());
        }
        let canonical = canonical_search_root
            .canonicalize()
            .map_err(|_| ProjectConfigLocationError::NotCanonical)?;
        if canonical != canonical_search_root {
            return Err(ProjectConfigLocationError::NotCanonical.into());
        }
        let key = utils_key(stable_identity);
        Ok(Self {
            canonical_search_root,
            key,
        })
    }

    pub fn search_root(&self) -> &Path {
        &self.canonical_search_root
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

fn utils_key(stable_identity: &[u8]) -> String {
    let mut hash = 1469598103934665603_u64;
    for byte in b"aemeath.config-project.v1\0".iter().chain(stable_identity) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("cfg-{hash:016x}")
}

#[derive(Debug, Clone)]
pub struct PreparedProjectConfigData {
    pub(crate) location: ProjectConfigLocationData,
    pub(crate) config: Config,
    pub(crate) snapshot: ConfigSnapshot,
}

impl PreparedProjectConfigData {
    pub fn location(&self) -> &ProjectConfigLocationData {
        &self.location
    }

    pub fn snapshot(&self) -> &ConfigSnapshot {
        &self.snapshot
    }

    pub fn memory_config(&self) -> &MemoryConfig {
        self.snapshot.memory()
    }
}

#[derive(Debug, Clone)]
pub struct PreparedConfigUpdateData {
    pub(crate) project_key: String,
    pub(crate) config: Config,
    pub(crate) override_patch: ConfigPatch,
    pub(crate) snapshot: ConfigSnapshot,
    pub(crate) fields: Vec<ConfigFieldData>,
}

impl PreparedConfigUpdateData {
    pub fn snapshot(&self) -> &ConfigSnapshot {
        &self.snapshot
    }

    pub fn memory_config(&self) -> &MemoryConfig {
        self.snapshot.memory()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigPersistError {
    Serialization,
    Io,
    PermissionDenied,
    UnsupportedDurability,
    CorruptTransaction,
}

impl std::fmt::Display for ConfigPersistError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialization => write!(formatter, "配置序列化失败"),
            Self::Io => write!(formatter, "配置写入 IO 失败"),
            Self::PermissionDenied => write!(formatter, "配置写入权限被拒绝"),
            Self::UnsupportedDurability => write!(formatter, "不支持的持久化模式"),
            Self::CorruptTransaction => write!(formatter, "配置事务文件损坏"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigCommitWarningData {
    PreviousPromotionPending,
    JournalCleanupPending,
}

#[derive(Debug, Clone)]
pub struct ReadyConfigCommitData {
    pub(crate) config: Config,
    pub(crate) snapshot: ConfigSnapshot,
    pub(crate) fields: Vec<ConfigFieldData>,
    pub(crate) warning: Option<ConfigCommitWarningData>,
}

impl ReadyConfigCommitData {
    pub fn snapshot(&self) -> &ConfigSnapshot {
        &self.snapshot
    }

    pub fn warning(&self) -> Option<ConfigCommitWarningData> {
        self.warning
    }
}

#[derive(Debug, Clone)]
pub enum ConfigPersistOutcomeData {
    NotCommitted(ConfigPersistError),
    Committed(Box<ReadyConfigCommitData>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigError {
    Load(String),
    InvalidLocation(ProjectConfigLocationError),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(message) => write!(formatter, "{message}"),
            Self::InvalidLocation(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
#[path = "domain_tests.rs"]
mod tests;

// ─── DomainError 折叠层（跨界唯一错误）────────────────────────────────

impl From<ConfigUpdateError> for share::error::DomainError {
    fn from(inner: ConfigUpdateError) -> Self {
        let category = match &inner {
            ConfigUpdateError::Persist(_) => share::error::ErrorCategory::Storage,
            ConfigUpdateError::Invalid(_) => share::error::ErrorCategory::Invalid,
        };
        share::error::DomainError::from_parts("config", category, inner.to_string())
    }
}

impl From<ProjectConfigLocationError> for share::error::DomainError {
    fn from(inner: ProjectConfigLocationError) -> Self {
        share::error::DomainError::from_parts(
            "config",
            share::error::ErrorCategory::Invalid,
            inner.to_string(),
        )
    }
}

impl From<ConfigError> for share::error::DomainError {
    fn from(inner: ConfigError) -> Self {
        let category = match &inner {
            ConfigError::Load(_) => share::error::ErrorCategory::Storage,
            ConfigError::InvalidLocation(_) => share::error::ErrorCategory::Invalid,
        };
        share::error::DomainError::from_parts("config", category, inner.to_string())
    }
}
