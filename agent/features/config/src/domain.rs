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
pub enum ConfigField {
    Model,
    PermissionMode,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigChangeCause {
    ClientUpdate,
    ProjectCommit,
    FileReload,
}

#[derive(Debug, Clone)]
pub struct ConfigChangeSet {
    pub cause: ConfigChangeCause,
    pub fields: Vec<ConfigField>,
    pub snapshot: ConfigSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigRefreshError {
    Io,
    Parse,
    Invalid,
}

#[derive(Debug, Clone)]
pub enum ConfigRefreshOutcome {
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
pub struct ConfigSubscription {
    pub initial: ConfigSnapshot,
    pub changes: watch::Receiver<ConfigSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigQueryError {
    Unavailable,
}

#[derive(Debug, Clone)]
pub enum ConfigUpdate {
    SetModel { model: String },
    SetPermissionMode { mode: PermissionModeConfig },
    SetMemoryConfig { config: MemoryConfig },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUpdateError {
    Invalid(String),
    Persist(ConfigPersistError),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectConfigLocation {
    canonical_search_root: PathBuf,
    key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectConfigLocationError {
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

impl ProjectConfigLocation {
    pub fn try_from_project_identity(
        canonical_search_root: PathBuf,
        stable_identity: &[u8],
    ) -> Result<Self, ProjectConfigLocationError> {
        if !canonical_search_root.is_absolute() {
            return Err(ProjectConfigLocationError::NotAbsolute);
        }
        if stable_identity.is_empty() {
            return Err(ProjectConfigLocationError::EmptyIdentity);
        }
        let canonical = canonical_search_root
            .canonicalize()
            .map_err(|_| ProjectConfigLocationError::NotCanonical)?;
        if canonical != canonical_search_root {
            return Err(ProjectConfigLocationError::NotCanonical);
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
pub struct PreparedProjectConfig {
    pub(crate) location: ProjectConfigLocation,
    pub(crate) config: Config,
    pub(crate) snapshot: ConfigSnapshot,
}

impl PreparedProjectConfig {
    pub fn location(&self) -> &ProjectConfigLocation {
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
pub struct PreparedConfigUpdate {
    pub(crate) project_key: String,
    pub(crate) config: Config,
    pub(crate) override_patch: ConfigPatch,
    pub(crate) snapshot: ConfigSnapshot,
    pub(crate) fields: Vec<ConfigField>,
}

impl PreparedConfigUpdate {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigCommitWarning {
    PreviousPromotionPending,
    JournalCleanupPending,
}

#[derive(Debug, Clone)]
pub struct ReadyConfigCommit {
    pub(crate) config: Config,
    pub(crate) snapshot: ConfigSnapshot,
    pub(crate) fields: Vec<ConfigField>,
    pub(crate) warning: Option<ConfigCommitWarning>,
}

impl ReadyConfigCommit {
    pub fn snapshot(&self) -> &ConfigSnapshot {
        &self.snapshot
    }

    pub fn warning(&self) -> Option<ConfigCommitWarning> {
        self.warning
    }
}

#[derive(Debug, Clone)]
pub enum ConfigPersistOutcome {
    NotCommitted(ConfigPersistError),
    Committed(Box<ReadyConfigCommit>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
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
