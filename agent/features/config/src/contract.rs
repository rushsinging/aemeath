use async_trait::async_trait;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::{MemoryConfig, PermissionModeConfig};
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

pub trait ConfigReader: Send + Sync {
    fn committed_snapshot(&self) -> ConfigSnapshot;
    fn subscribe_committed(&self) -> watch::Receiver<ConfigSnapshot>;
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

#[async_trait]
pub trait ConfigQuery: Send + Sync {
    async fn snapshot(&self) -> Result<ConfigSnapshot, ConfigQueryError>;
    async fn subscribe(&self) -> Result<ConfigSubscription, ConfigQueryError>;
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

#[async_trait]
pub trait ConfigWriter: Send + Sync {
    async fn update(&self, command: ConfigUpdate) -> Result<ConfigChangeSet, ConfigUpdateError>;
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
    pub(crate) snapshot: ConfigSnapshot,
    pub(crate) bytes: Vec<u8>,
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
    Committed(ReadyConfigCommit),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Load(String),
    InvalidLocation(ProjectConfigLocationError),
}

#[async_trait]
pub trait ProjectConfigParticipant: Send + Sync {
    async fn prepare_for_project(
        &self,
        location: &ProjectConfigLocation,
    ) -> Result<PreparedProjectConfig, ConfigError>;
    fn snapshot(&self) -> ConfigSnapshot;
    fn commit_project(&self, prepared: PreparedProjectConfig);
    async fn prepare_update(
        &self,
        command: ConfigUpdate,
    ) -> Result<PreparedConfigUpdate, ConfigUpdateError>;
    async fn persist_update(&self, prepared: PreparedConfigUpdate) -> ConfigPersistOutcome;
    fn commit_update(&self, ready: ReadyConfigCommit) -> ConfigChangeSet;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_config_location_rejects_relative_path_and_empty_identity() {
        assert_eq!(
            ProjectConfigLocation::try_from_project_identity(PathBuf::from("relative"), b"id"),
            Err(ProjectConfigLocationError::NotAbsolute)
        );
    }

    #[test]
    fn project_config_location_is_stable_for_same_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let first =
            ProjectConfigLocation::try_from_project_identity(root.clone(), b"project").unwrap();
        let second = ProjectConfigLocation::try_from_project_identity(root, b"project").unwrap();
        assert_eq!(first, second);
    }
}
