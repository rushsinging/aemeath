use crate::tui::model::conversation::workspace::WorktreeKind;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceIntent {
    SetCurrent {
        cwd: String,
        worktree: Option<String>,
    },
    ApplySnapshot {
        path_base: Option<String>,
        workspace_root: Option<String>,
    },
    ApplyMetadata {
        root: String,
        revision: u64,
        branch: Option<String>,
        kind: WorktreeKind,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceChange {
    CurrentChanged,
    SnapshotApplied { root: Option<String>, revision: u64 },
    MetadataApplied { revision: u64 },
    MetadataDiscarded { root: String, revision: u64 },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceProvider {
    cwd: Option<String>,
    worktree: Option<String>,
    path_base: Option<String>,
    workspace_root: Option<String>,
    branch: Option<String>,
    kind: WorktreeKind,
    revision: u64,
}

impl WorkspaceProvider {
    pub(crate) fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    pub(crate) fn worktree(&self) -> Option<&str> {
        self.worktree.as_deref()
    }

    pub(crate) fn path_base(&self) -> Option<&str> {
        self.path_base.as_deref()
    }

    pub(crate) fn workspace_root(&self) -> Option<&str> {
        self.workspace_root.as_deref()
    }

    pub(crate) fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub(crate) fn kind(&self) -> WorktreeKind {
        self.kind
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn apply(&mut self, intent: WorkspaceIntent) -> WorkspaceChange {
        match intent {
            WorkspaceIntent::SetCurrent { cwd, worktree } => {
                self.cwd = Some(cwd);
                self.worktree = worktree;
                WorkspaceChange::CurrentChanged
            }
            WorkspaceIntent::ApplySnapshot {
                path_base,
                workspace_root,
            } => {
                self.path_base = path_base;
                self.workspace_root = workspace_root;
                self.branch = None;
                self.kind = WorktreeKind::Unknown;
                self.revision = self.revision.wrapping_add(1);
                WorkspaceChange::SnapshotApplied {
                    root: self.workspace_root.clone(),
                    revision: self.revision,
                }
            }
            WorkspaceIntent::ApplyMetadata {
                root,
                revision,
                branch,
                kind,
            } if self.workspace_root.as_deref() == Some(root.as_str())
                && self.revision == revision =>
            {
                self.branch = branch;
                self.kind = kind;
                WorkspaceChange::MetadataApplied { revision }
            }
            WorkspaceIntent::ApplyMetadata { root, revision, .. } => {
                WorkspaceChange::MetadataDiscarded { root, revision }
            }
        }
    }
}

#[cfg(test)]
#[path = "workspace_provider_tests.rs"]
mod tests;
