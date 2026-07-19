use super::{MemoryEntry, MemoryLayer};
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
};
use thiserror::Error;

const PROJECT_KEY_DOMAIN: &[u8] = b"aemeath.memory.project-key.v2\0";

/// Opaque project identity for Memory layers.
///
/// Carries the versioned hash key (used as the Storage dataset segment) and the
/// cwd-derived file stem used to locate legacy memory files. Neither field is a
/// filesystem path: the hash is a fixed-length digest and the legacy stem is a
/// sanitized, opaque identifier (leading `/` stripped, remaining `/` replaced
/// by `-`) so no raw path crosses the Memory boundary.
///
/// **Identity** is defined solely by the hash key — two git worktrees sharing
/// the same `.git` directory derive the same key and are therefore equal, even
/// though their legacy file stems differ. The legacy stem is auxiliary data for
/// resolving predecessor files, not part of the identity.
#[derive(Debug, Clone)]
pub struct ProjectMemoryKey {
    key: String,
    legacy_project_name: String,
}

impl PartialEq for ProjectMemoryKey {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for ProjectMemoryKey {}

impl Hash for ProjectMemoryKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

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
        Ok(Self {
            key: format!("v2_{digest}"),
            legacy_project_name: legacy_project_file_name(initial_cwd),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.key
    }

    /// Returns the cwd-derived file stem used by the legacy memory file layout
    /// (e.g. `Users-guoyuqi-work-aemeath`). This is **not** a filesystem path —
    /// it is a sanitized, opaque identifier derived from the original cwd at
    /// key-derivation time, matching the predecessor flat-file layout
    /// (`_global.json` / `{stem}.json` / `{stem}_archive.json`).
    pub(crate) fn legacy_project_name(&self) -> &str {
        &self.legacy_project_name
    }
}

fn legacy_project_file_name(cwd: &str) -> String {
    cwd.trim_start_matches('/').replace('/', "-")
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
    fn legacy_project_name_matches_predecessor_file_stem() {
        let key = ProjectMemoryKey::derive("/Users/guoyuqi/work/aemeath", None).unwrap();
        assert_eq!(key.legacy_project_name(), "Users-guoyuqi-work-aemeath");

        // Git projects share the key but use their *own* cwd for the legacy stem.
        let main = ProjectMemoryKey::derive("/repo", Some("/repo/.git")).unwrap();
        let worktree =
            ProjectMemoryKey::derive("/repo/.worktrees/feat", Some("/repo/.git")).unwrap();
        assert_eq!(main, worktree, "keys are equal (shared git identity)");
        assert_eq!(main.legacy_project_name(), "repo");
        assert_eq!(
            worktree.legacy_project_name(),
            "repo-.worktrees-feat",
            "legacy stem uses the *caller* cwd, not the shared git dir"
        );
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
