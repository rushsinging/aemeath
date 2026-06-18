use std::path::{Path, PathBuf};

use share::session_types::{PersistedWorkspaceContext, PersistedWorkspaceFrame};

use crate::business::git_ops::GitWorktreeOps;
use crate::business::workspace_types::{WorkspaceError, WorkspaceFrame};

const DEFAULT_WORKTREE_BASE: &str = "main";
const DEFAULT_WORKTREE_DIR: &str = ".worktrees";

pub struct WorkspaceState {
    pub initial_cwd: PathBuf,
    pub working_root: PathBuf,
    pub path_base: PathBuf,
    pub stack: Vec<WorkspaceFrame>,
}

impl WorkspaceState {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            initial_cwd: cwd.clone(),
            working_root: cwd.clone(),
            path_base: cwd,
            stack: Vec::new(),
        }
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

pub fn set_cwd(
    state: &mut WorkspaceState,
    git: &dyn GitWorktreeOps,
    path: PathBuf,
) -> Result<(), WorkspaceError> {
    if let Ok(root) = git.show_toplevel(&path) {
        state.working_root = root;
    }
    state.path_base = path;
    Ok(())
}

/// Canonicalize `target` and verify it lives in the same repo as `state.working_root`.
/// Returns `(canonical_path, worktree_root)` on success.
fn validate_in_repo(
    state: &WorkspaceState,
    git: &dyn GitWorktreeOps,
    target: &Path,
) -> Result<(PathBuf, PathBuf), WorkspaceError> {
    let canonical = target
        .canonicalize()
        .map_err(|_| WorkspaceError::PathNotFound(target.to_path_buf()))?;
    let worktree_root = git.show_toplevel(&canonical).map_err(WorkspaceError::Git)?;
    if let Ok(a) = git.git_common_dir(&state.working_root) {
        if let Ok(b) = git.git_common_dir(&worktree_root) {
            if a != b {
                return Err(WorkspaceError::RepoMismatch {
                    path: worktree_root,
                    repo_root: state.working_root.clone(),
                });
            }
        }
    }
    Ok((canonical, worktree_root))
}

pub fn enter(
    state: &mut WorkspaceState,
    git: &dyn GitWorktreeOps,
    path: Option<PathBuf>,
    branch: Option<String>,
) -> Result<WorkspaceFrame, WorkspaceError> {
    if !state.stack.is_empty() {
        if !git.in_worktree(&state.path_base) {
            state.stack.clear(); // 残栈自愈（refs #96）
        } else {
            return Err(WorkspaceError::NestedWorktree {
                current_working_root: state.working_root.clone(),
                current_path_base: state.path_base.clone(),
            });
        }
    }
    let target = resolve_worktree_path(state, path, branch.as_deref())?;
    if !target.exists() {
        let b = branch
            .filter(|v| !v.trim().is_empty())
            .ok_or(WorkspaceError::MissingPathAndBranch)?;
        git.worktree_add(&state.working_root, &target, &b, DEFAULT_WORKTREE_BASE)
            .map_err(WorkspaceError::Git)?;
    }
    let (canonical, worktree_root) = validate_in_repo(state, git, &target)?;
    let frame = WorkspaceFrame {
        path_base: state.path_base.clone(),
        working_root: state.working_root.clone(),
    };
    state.stack.push(frame.clone());
    state.working_root = worktree_root;
    state.path_base = canonical;
    Ok(frame)
}

/// Switch the workspace to `path` without pushing a stack frame.
/// Validates that the path exists and belongs to the same repo as the current root.
pub fn switch_to(
    state: &mut WorkspaceState,
    git: &dyn GitWorktreeOps,
    path: PathBuf,
) -> Result<(), WorkspaceError> {
    let (canonical, worktree_root) = validate_in_repo(state, git, &path)?;
    state.working_root = worktree_root;
    state.path_base = canonical;
    Ok(())
}

pub fn exit(state: &mut WorkspaceState) -> Result<WorkspaceFrame, WorkspaceError> {
    match state.stack.pop() {
        Some(prev) => {
            state.working_root = prev.working_root.clone();
            state.path_base = prev.path_base.clone();
            Ok(prev)
        }
        None => Err(WorkspaceError::EmptyStack),
    }
}

pub fn snapshot(state: &WorkspaceState) -> PersistedWorkspaceContext {
    PersistedWorkspaceContext {
        path_base: state.path_base.display().to_string(),
        working_root: state.working_root.display().to_string(),
        context_stack: state
            .stack
            .iter()
            .map(|f| PersistedWorkspaceFrame {
                path_base: f.path_base.display().to_string(),
                working_root: f.working_root.display().to_string(),
            })
            .collect(),
    }
}

pub fn restore(
    state: &mut WorkspaceState,
    dto: &PersistedWorkspaceContext,
) -> Result<(), WorkspaceError> {
    let path_base = PathBuf::from(&dto.path_base);
    let working_root = PathBuf::from(&dto.working_root);
    if !path_base.exists() {
        return Err(WorkspaceError::RestoreInvalidPath(path_base));
    }
    if !working_root.exists() {
        return Err(WorkspaceError::RestoreInvalidPath(working_root));
    }
    let stack = dto
        .context_stack
        .iter()
        .map(|e| WorkspaceFrame {
            path_base: PathBuf::from(&e.path_base),
            working_root: PathBuf::from(&e.working_root),
        })
        .collect();
    state.path_base = path_base;
    state.working_root = working_root;
    state.stack = stack;
    Ok(())
}

#[cfg(test)]
#[path = "workspace_state_tests.rs"]
mod tests;
