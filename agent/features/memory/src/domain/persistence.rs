use super::{MemoryEntry, MemoryLayer};
use std::collections::HashSet;
use thiserror::Error;

const PROJECT_KEY_DOMAIN: &[u8] = b"aemeath.memory.project-key.v2\0";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectMemoryKey(String);

impl ProjectMemoryKey {
    pub fn derive(
        initial_cwd: &str,
        git_common_dir: Option<&str>,
    ) -> Result<Self, MemoryOpenError> {
        if initial_cwd.is_empty() {
            return Err(MemoryOpenError::InvalidProjectIdentity);
        }
        let (kind, identity) = match git_common_dir {
            Some("") => return Err(MemoryOpenError::InvalidProjectIdentity),
            Some(common_dir) => (b"git".as_slice(), common_dir.as_bytes()),
            None => (b"non-git".as_slice(), initial_cwd.as_bytes()),
        };
        let digest = utils::stable_sha256_hex(PROJECT_KEY_DOMAIN, &[kind, identity]);
        Ok(Self(format!("v2_{digest}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryDataset {
    layer: MemoryLayer,
    active: Vec<MemoryEntry>,
    archive: Vec<MemoryEntry>,
}

impl MemoryDataset {
    pub fn empty(layer: MemoryLayer) -> Self {
        Self {
            layer,
            active: Vec::new(),
            archive: Vec::new(),
        }
    }

    pub fn new(
        layer: MemoryLayer,
        active: Vec<MemoryEntry>,
        archive: Vec<MemoryEntry>,
    ) -> Result<Self, MemoryOpenError> {
        let mut ids = HashSet::new();
        for entry in active.iter().chain(&archive) {
            if entry.layer != layer || entry.content.trim().is_empty() || !ids.insert(entry.id) {
                return Err(MemoryOpenError::CorruptDataset {
                    message: "记忆数据集违反层级、内容或 ID 不变量".to_string(),
                });
            }
        }
        Ok(Self {
            layer,
            active,
            archive,
        })
    }

    pub fn layer(&self) -> MemoryLayer {
        self.layer
    }
    pub fn active(&self) -> &[MemoryEntry] {
        &self.active
    }
    pub fn archive(&self) -> &[MemoryEntry] {
        &self.archive
    }
    pub(crate) fn active_mut(&mut self) -> &mut Vec<MemoryEntry> {
        &mut self.active
    }
    pub(crate) fn archive_mut(&mut self) -> &mut Vec<MemoryEntry> {
        &mut self.archive
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MemoryOpenError {
    #[error("项目身份无效")]
    InvalidProjectIdentity,
    #[error("记忆数据集损坏: {message}")]
    CorruptDataset { message: String },
    #[error("不支持的记忆 schema 版本: {version}")]
    UnsupportedSchema { version: u32 },
    #[error("新旧记忆 key 同时存在")]
    LegacyKeyConflict,
    #[error("记忆持久化打开失败")]
    Storage,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryCategory, MemoryId, MemorySource};

    fn entry(id: MemoryId, layer: MemoryLayer, content: &str) -> MemoryEntry {
        MemoryEntry::new(
            id,
            1,
            layer,
            MemoryCategory::Fact,
            content,
            MemorySource::User,
        )
        .unwrap()
    }

    #[test]
    fn project_key_is_versioned_stable_and_does_not_expose_paths() {
        let first = ProjectMemoryKey::derive("/repo", Some("/repo/.git")).unwrap();
        let second = ProjectMemoryKey::derive("/other-worktree", Some("/repo/.git")).unwrap();
        assert_eq!(first, second);
        assert!(first.as_str().starts_with("v2_"));
        assert!(!first.as_str().contains("repo"));
    }

    #[test]
    fn non_git_project_key_uses_initial_cwd_and_rejects_empty_identity() {
        assert_ne!(
            ProjectMemoryKey::derive("/a", None).unwrap(),
            ProjectMemoryKey::derive("/b", None).unwrap()
        );
        assert!(ProjectMemoryKey::derive("", None).is_err());
        assert!(ProjectMemoryKey::derive("/project", Some("")).is_err());
    }

    #[test]
    fn dataset_rejects_duplicate_ids_across_active_and_archive() {
        let id = MemoryId::now_v7();
        assert!(MemoryDataset::new(
            MemoryLayer::Project,
            vec![entry(id, MemoryLayer::Project, "active")],
            vec![entry(id, MemoryLayer::Project, "archive")]
        )
        .is_err());
    }

    #[test]
    fn dataset_rejects_entries_from_another_layer() {
        assert!(MemoryDataset::new(
            MemoryLayer::Project,
            vec![entry(MemoryId::now_v7(), MemoryLayer::Global, "global")],
            vec![]
        )
        .is_err());
    }
}
