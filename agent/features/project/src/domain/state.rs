use std::path::{Path, PathBuf};

use share::session_types::{
    PersistedWorkspaceContext, PersistedWorkspaceFrame, ProjectIdentity, WorkspaceId, WorktreeKind,
};

use crate::domain::git::{GitWorktreeOps, RepositoryProbe};
use crate::domain::types::{WorkspaceError, WorkspaceFrame, WorkspaceRestoreError};

const DEFAULT_WORKTREE_BASE: &str = "main";
const DEFAULT_WORKTREE_DIR: &str = ".worktrees";

#[derive(Clone)]
pub struct WorkspaceState {
    pub project_identity: ProjectIdentity,
    pub workspace_root: PathBuf,
    pub path_base: PathBuf,
    pub worktree_kind: WorktreeKind,
    pub stack: Vec<WorkspaceFrame>,
}

impl WorkspaceState {
    /// Test constructor.
    #[cfg(test)]
    pub fn new(cwd: PathBuf) -> Self {
        Self::from_verified(
            ProjectIdentity {
                initial_cwd: cwd.display().to_string(),
                git_common_dir: Some(cwd.join(".git").display().to_string()),
            },
            cwd.clone(),
            cwd,
            WorktreeKind::Primary,
        )
    }

    pub fn from_verified(
        project_identity: ProjectIdentity,
        workspace_root: PathBuf,
        path_base: PathBuf,
        worktree_kind: WorktreeKind,
    ) -> Self {
        Self {
            project_identity,
            workspace_root,
            path_base,
            worktree_kind,
            stack: Vec::new(),
        }
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        WorkspaceId::derive(
            &self.project_identity,
            &self.workspace_root.display().to_string(),
        )
    }
    pub fn resolve(&self, rel: &Path) -> PathBuf {
        if rel.is_absolute() {
            rel.to_path_buf()
        } else {
            self.path_base.join(rel)
        }
    }
}

fn sanitize_branch_for_path(branch: &str) -> Result<String, WorkspaceError> {
    let mut s = String::new();
    let mut last_dash = false;
    for ch in branch.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            s.push(ch);
            last_dash = false;
        } else if !last_dash {
            s.push('-');
            last_dash = true;
        }
    }
    let s = s.trim_matches(|c| matches!(c, '.' | '_' | '-')).to_string();
    if s.is_empty() {
        return Err(WorkspaceError::InvalidBranch);
    }
    Ok(s)
}

fn resolve_worktree_path(
    state: &WorkspaceState,
    path: Option<PathBuf>,
    branch: Option<&str>,
) -> Result<PathBuf, WorkspaceError> {
    match path {
        Some(p) if p.is_absolute() => Ok(p),
        Some(p) => Ok(state.path_base.join(p)),
        None => match branch {
            Some(b) if !b.trim().is_empty() => Ok(state
                .path_base
                .join(DEFAULT_WORKTREE_DIR)
                .join(sanitize_branch_for_path(b)?)),
            _ => Err(WorkspaceError::MissingPathAndBranch),
        },
    }
}

pub fn change_directory(state: &mut WorkspaceState, path: PathBuf) -> Result<(), WorkspaceError> {
    let canonical = path
        .canonicalize()
        .map_err(|_| WorkspaceError::PathNotFound(path.clone()))?;
    if !canonical.is_dir() {
        return Err(WorkspaceError::NotDirectory(canonical));
    }
    let canonical_root = state
        .workspace_root
        .canonicalize()
        .map_err(|_| WorkspaceError::PathNotFound(state.workspace_root.clone()))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(WorkspaceError::PathOutsideWorkspaceRoot {
            path: canonical,
            root: canonical_root,
        });
    }
    state.path_base = canonical;
    Ok(())
}

/// Canonicalize `target` and verify it lives in the same repo as `state.workspace_root`.
/// Returns `(canonical_path, worktree_root, actual_kind)` on success.
fn validate_in_repo(
    state: &WorkspaceState,
    git: &dyn GitWorktreeOps,
    target: &Path,
) -> Result<(PathBuf, PathBuf, WorktreeKind), WorkspaceError> {
    let canonical = target
        .canonicalize()
        .map_err(|_| WorkspaceError::PathNotFound(target.to_path_buf()))?;
    if !canonical.is_dir() {
        return Err(WorkspaceError::NotDirectory(canonical));
    }
    let worktree_root = git
        .show_toplevel(&canonical)
        .map_err(WorkspaceError::GitOperationFailed)?;
    let probe = git
        .probe_repository(&canonical)
        .map_err(WorkspaceError::GitProbeFailed)?;
    match probe {
        RepositoryProbe::Git {
            canonical_top_level,
            canonical_common_dir,
            worktree_kind,
        } if canonical_top_level == worktree_root
            && state
                .project_identity
                .git_common_dir
                .as_deref()
                .is_some_and(|expected| canonical_common_dir == Path::new(expected)) =>
        {
            Ok((canonical, worktree_root, worktree_kind))
        }
        _ => Err(WorkspaceError::RepoMismatch {
            path: worktree_root,
            repo_root: state.workspace_root.clone(),
        }),
    }
}

pub fn enter(
    state: &mut WorkspaceState,
    git: &dyn GitWorktreeOps,
    path: Option<PathBuf>,
    branch: Option<String>,
) -> Result<WorkspaceFrame, WorkspaceError> {
    if state.worktree_kind == WorktreeKind::NonGit {
        return Err(WorkspaceError::UnsupportedForNonGit);
    }
    if !state.stack.is_empty() {
        match git
            .is_linked_worktree(&state.path_base)
            .map_err(WorkspaceError::GitOperationFailed)?
        {
            false => state.stack.clear(),
            true => {
                return Err(WorkspaceError::NestedWorktree {
                    current_workspace_root: state.workspace_root.clone(),
                    current_path_base: state.path_base.clone(),
                });
            }
        }
    }
    let target = resolve_worktree_path(state, path, branch.as_deref())?;
    if !target.exists() {
        let b = branch
            .filter(|v| !v.trim().is_empty())
            .ok_or(WorkspaceError::MissingPathAndBranch)?;
        git.worktree_add(&state.workspace_root, &target, &b, DEFAULT_WORKTREE_BASE)
            .map_err(WorkspaceError::GitOperationFailed)?;
    }
    let (canonical, worktree_root, worktree_kind) = validate_in_repo(state, git, &target)?;
    if worktree_kind != WorktreeKind::Linked {
        return Err(WorkspaceError::RepoMismatch {
            path: worktree_root,
            repo_root: state.workspace_root.clone(),
        });
    }
    let frame = WorkspaceFrame {
        path_base: state.path_base.clone(),
        workspace_root: state.workspace_root.clone(),
        worktree_kind: state.worktree_kind,
    };
    state.stack.push(frame.clone());
    state.workspace_root = worktree_root;
    state.path_base = canonical;
    state.worktree_kind = worktree_kind;
    Ok(frame)
}

/// Switch the workspace to `path` without pushing a stack frame.
/// Validates that the path exists and belongs to the same repo as the current root.
pub fn switch_to(
    state: &mut WorkspaceState,
    git: &dyn GitWorktreeOps,
    path: PathBuf,
) -> Result<(), WorkspaceError> {
    if state.worktree_kind == WorktreeKind::NonGit {
        return Err(WorkspaceError::UnsupportedForNonGit);
    }
    let (canonical, worktree_root, kind) = validate_in_repo(state, git, &path)?;
    state.workspace_root = worktree_root;
    state.path_base = canonical;
    state.worktree_kind = kind;
    Ok(())
}

pub fn exit(
    state: &mut WorkspaceState,
    git: &dyn GitWorktreeOps,
) -> Result<WorkspaceFrame, WorkspaceError> {
    if state.worktree_kind == WorktreeKind::NonGit {
        return Err(WorkspaceError::UnsupportedForNonGit);
    }
    let prev = state
        .stack
        .last()
        .cloned()
        .ok_or(WorkspaceError::EmptyStack)?;
    let (canonical, worktree_root, worktree_kind) = validate_in_repo(state, git, &prev.path_base)?;
    if canonical != prev.path_base
        || worktree_root != prev.workspace_root
        || worktree_kind != prev.worktree_kind
    {
        return Err(WorkspaceError::RepoMismatch {
            path: canonical,
            repo_root: prev.workspace_root,
        });
    }
    state.stack.pop();
    state.workspace_root = worktree_root;
    state.path_base = canonical;
    state.worktree_kind = worktree_kind;
    Ok(prev)
}

pub fn snapshot(state: &WorkspaceState) -> PersistedWorkspaceContext {
    PersistedWorkspaceContext {
        workspace_id: state.workspace_id(),
        project_identity: state.project_identity.clone(),
        path_base: state.path_base.display().to_string(),
        workspace_root: state.workspace_root.display().to_string(),
        worktree_kind: state.worktree_kind,
        context_stack: state
            .stack
            .iter()
            .map(|f| PersistedWorkspaceFrame {
                path_base: f.path_base.display().to_string(),
                workspace_root: f.workspace_root.display().to_string(),
                worktree_kind: f.worktree_kind,
            })
            .collect(),
    }
}

#[must_use]
pub struct PreparedWorkspaceRestore {
    candidate: WorkspaceState,
}

impl PreparedWorkspaceRestore {
    pub fn project_identity(&self) -> &ProjectIdentity {
        &self.candidate.project_identity
    }
}

impl std::fmt::Debug for PreparedWorkspaceRestore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedWorkspaceRestore")
            .finish_non_exhaustive()
    }
}

fn restore_path(raw: &str) -> Result<PathBuf, WorkspaceRestoreError> {
    let path = PathBuf::from(raw);
    if raw.is_empty() || !path.exists() || !path.is_dir() {
        return Err(WorkspaceRestoreError::PathNotFound {
            path: path.to_string_lossy().into_owned(),
        });
    }
    path.canonicalize()
        .map_err(|_| WorkspaceRestoreError::PathNotFound {
            path: path.to_string_lossy().into_owned(),
        })
}

fn validate_containment(path: &Path, root: &Path) -> Result<(), WorkspaceRestoreError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(WorkspaceRestoreError::PathOutsideWorkspaceRoot {
            path: path.to_string_lossy().into_owned(),
            root: root.to_string_lossy().into_owned(),
        })
    }
}

fn probe_restore(
    git: &dyn GitWorktreeOps,
    path: &Path,
) -> Result<RepositoryProbe, WorkspaceRestoreError> {
    git.probe_repository(path)
        .map_err(WorkspaceRestoreError::GitProbeFailed)
}

fn validate_git_location(
    git: &dyn GitWorktreeOps,
    path: &Path,
    expected_root: Option<&Path>,
    expected_common: &Path,
    expected_kind: Option<WorktreeKind>,
) -> Result<(), WorkspaceRestoreError> {
    match probe_restore(git, path)? {
        RepositoryProbe::Git {
            canonical_top_level,
            canonical_common_dir,
            worktree_kind,
        } if canonical_common_dir == expected_common
            && expected_root.is_none_or(|root| canonical_top_level == root)
            && expected_kind.is_none_or(|kind| worktree_kind == kind) =>
        {
            Ok(())
        }
        _ => Err(WorkspaceRestoreError::RepositoryMismatch),
    }
}

pub fn prepare_restore(
    _live_state: &WorkspaceState,
    dto: &PersistedWorkspaceContext,
    git: &dyn GitWorktreeOps,
) -> Result<PreparedWorkspaceRestore, WorkspaceRestoreError> {
    if dto.project_identity.initial_cwd.is_empty() {
        return Err(WorkspaceRestoreError::InvalidProjectIdentity);
    }

    let initial_cwd = restore_path(&dto.project_identity.initial_cwd)?;
    let workspace_root = restore_path(&dto.workspace_root)?;
    let path_base = restore_path(&dto.path_base)?;
    validate_containment(&path_base, &workspace_root)?;

    let mut stack = Vec::with_capacity(dto.context_stack.len());
    for persisted in &dto.context_stack {
        let frame_root = restore_path(&persisted.workspace_root)?;
        let frame_base = restore_path(&persisted.path_base)?;
        validate_containment(&frame_base, &frame_root)?;
        stack.push(WorkspaceFrame {
            path_base: frame_base,
            workspace_root: frame_root,
            worktree_kind: persisted.worktree_kind,
        });
    }

    let canonical_identity = match dto.project_identity.git_common_dir.as_deref() {
        Some(common) if !common.is_empty() => {
            let common = PathBuf::from(common);
            if !common.is_absolute()
                || dto.worktree_kind == WorktreeKind::NonGit
                || stack.len() > 1
                || stack
                    .iter()
                    .any(|frame| frame.worktree_kind != WorktreeKind::Primary)
                || (!stack.is_empty() && dto.worktree_kind != WorktreeKind::Linked)
            {
                return Err(WorkspaceRestoreError::InvalidStackShape);
            }

            validate_git_location(git, &initial_cwd, None, &common, None)?;
            validate_git_location(
                git,
                &workspace_root,
                Some(&workspace_root),
                &common,
                Some(dto.worktree_kind),
            )?;
            validate_git_location(
                git,
                &path_base,
                Some(&workspace_root),
                &common,
                Some(dto.worktree_kind),
            )?;
            for frame in &stack {
                validate_git_location(
                    git,
                    &frame.workspace_root,
                    Some(&frame.workspace_root),
                    &common,
                    Some(frame.worktree_kind),
                )?;
                validate_git_location(
                    git,
                    &frame.path_base,
                    Some(&frame.workspace_root),
                    &common,
                    Some(frame.worktree_kind),
                )?;
            }
            ProjectIdentity {
                initial_cwd: initial_cwd.to_string_lossy().into_owned(),
                git_common_dir: Some(common.to_string_lossy().into_owned()),
            }
        }
        Some(_) => return Err(WorkspaceRestoreError::InvalidProjectIdentity),
        None => {
            if dto.worktree_kind != WorktreeKind::NonGit
                || !stack.is_empty()
                || workspace_root != initial_cwd
            {
                return Err(WorkspaceRestoreError::InvalidStackShape);
            }
            if !matches!(probe_restore(git, &initial_cwd)?, RepositoryProbe::NonGit)
                || !matches!(probe_restore(git, &path_base)?, RepositoryProbe::NonGit)
            {
                return Err(WorkspaceRestoreError::RepositoryMismatch);
            }
            ProjectIdentity {
                initial_cwd: initial_cwd.to_string_lossy().into_owned(),
                git_common_dir: None,
            }
        }
    };

    let expected_id = WorkspaceId::derive(&canonical_identity, &workspace_root.to_string_lossy());
    if dto.workspace_id != expected_id {
        return Err(WorkspaceRestoreError::WorkspaceIdMismatch);
    }

    Ok(PreparedWorkspaceRestore {
        candidate: WorkspaceState {
            project_identity: canonical_identity,
            workspace_root,
            path_base,
            worktree_kind: dto.worktree_kind,
            stack,
        },
    })
}

pub fn commit_restore(state: &mut WorkspaceState, prepared: PreparedWorkspaceRestore) {
    *state = prepared.candidate;
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
