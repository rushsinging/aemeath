use std::path::{Path, PathBuf};

use share::session_types::{PersistedWorkspaceContext, PersistedWorkspaceFrame};

use crate::business::git_ops::GitWorktreeOps;
use crate::contract::{WorkspaceError, WorkspaceFrame};

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
    let root = git.show_toplevel(&path).unwrap_or_else(|_| path.clone());
    state.working_root = root;
    state.path_base = path;
    Ok(())
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
            return Err(WorkspaceError::NestedWorktree);
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
    let canonical = target
        .canonicalize()
        .map_err(|_| WorkspaceError::PathNotFound(target.clone()))?;
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
    let frame = WorkspaceFrame {
        path_base: state.path_base.clone(),
        working_root: state.working_root.clone(),
    };
    state.stack.push(frame.clone());
    set_cwd(state, git, canonical)?;
    Ok(frame)
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
mod tests {
    use super::*;
    use crate::business::git_ops::tests::FakeGit;

    fn st(cwd: &str) -> WorkspaceState {
        WorkspaceState::new(PathBuf::from(cwd))
    }

    #[test]
    fn init_consistent() {
        let s = st("/repo");
        assert_eq!(s.working_root, PathBuf::from("/repo"));
        assert_eq!(s.path_base, PathBuf::from("/repo"));
        assert!(s.stack.is_empty());
    }

    #[test]
    fn resolve_relative_uses_path_base() {
        let mut s = st("/repo");
        s.path_base = PathBuf::from("/repo/sub");
        assert_eq!(
            s.resolve(Path::new("a/b.rs")),
            PathBuf::from("/repo/sub/a/b.rs")
        );
        assert_eq!(s.resolve(Path::new("/abs/x")), PathBuf::from("/abs/x"));
    }

    #[test]
    fn exit_empty_stack_errors() {
        let mut s = st("/repo");
        assert_eq!(exit(&mut s), Err(WorkspaceError::EmptyStack));
    }

    #[test]
    fn exit_pops_and_restores() {
        let mut s = st("/repo");
        s.stack.push(WorkspaceFrame {
            path_base: "/prev".into(),
            working_root: "/prev".into(),
        });
        s.path_base = "/wt".into();
        s.working_root = "/wt".into();
        let prev = exit(&mut s).unwrap();
        assert_eq!(prev.path_base, PathBuf::from("/prev"));
        assert_eq!(s.path_base, PathBuf::from("/prev"));
    }

    #[test]
    fn set_cwd_detects_root() {
        let mut git = FakeGit::default();
        git.toplevel
            .insert(PathBuf::from("/repo/sub"), PathBuf::from("/repo"));
        let mut s = st("/repo");
        set_cwd(&mut s, &git, PathBuf::from("/repo/sub")).unwrap();
        assert_eq!(s.path_base, PathBuf::from("/repo/sub"));
        assert_eq!(s.working_root, PathBuf::from("/repo"));
    }

    #[test]
    fn snapshot_restore_roundtrip() {
        let mut s = st("/repo");
        s.path_base = "/repo/sub".into();
        s.stack.push(WorkspaceFrame {
            path_base: "/repo".into(),
            working_root: "/repo".into(),
        });
        let dto = snapshot(&s);
        let mut s2 = st("/tmp");
        // restore 校验路径存在：用真实存在的临时目录替换
        let dir = std::env::temp_dir();
        let dto2 = PersistedWorkspaceContext {
            path_base: dir.display().to_string(),
            working_root: dir.display().to_string(),
            context_stack: dto.context_stack.clone(),
        };
        restore(&mut s2, &dto2).unwrap();
        assert_eq!(s2.path_base, dir);
        assert_eq!(s2.stack.len(), 1);
    }

    #[test]
    fn restore_invalid_path_fails_whole() {
        let mut s = st("/repo");
        let bad = PersistedWorkspaceContext {
            path_base: "/definitely/not/here/xyz".into(),
            working_root: "/definitely/not/here/xyz".into(),
            context_stack: vec![],
        };
        assert!(matches!(
            restore(&mut s, &bad),
            Err(WorkspaceError::RestoreInvalidPath(_))
        ));
        // 状态未被部分修改
        assert_eq!(s.path_base, PathBuf::from("/repo"));
    }

    #[test]
    fn enter_rejects_nested_when_in_worktree() {
        let mut git = FakeGit::default();
        git.worktrees.insert(PathBuf::from("/repo")); // 当前 path_base 在 worktree 中
        let mut s = st("/repo");
        s.stack.push(WorkspaceFrame {
            path_base: "/prev".into(),
            working_root: "/prev".into(),
        });
        assert_eq!(
            enter(&mut s, &git, Some("/other".into()), None),
            Err(WorkspaceError::NestedWorktree)
        );
    }

    #[test]
    fn enter_missing_path_and_branch_errors() {
        let git = FakeGit::default();
        let mut s = st("/repo");
        assert_eq!(
            enter(&mut s, &git, None, None),
            Err(WorkspaceError::MissingPathAndBranch)
        );
    }

    #[test]
    fn enter_happy_path_pushes_frame_and_swaps_cwd() {
        use std::time::{SystemTime, UNIX_EPOCH};

        // Create a unique real temp dir so canonicalize() succeeds.
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let tmp = std::env::temp_dir().join(format!("aemeath_test_{}", nanos));
        std::fs::create_dir_all(&tmp).unwrap();
        let canonical_tmp = tmp.canonicalize().unwrap();

        // The worktree root that FakeGit reports for the temp dir.
        let worktree_root = PathBuf::from("/repo/wt");
        // A shared common dir value (same for both initial cwd and worktree root → same repo).
        let common = PathBuf::from("/repo/.git");

        let mut git = FakeGit::default();
        // show_toplevel: called twice — once inside enter (for canonical_tmp), once inside set_cwd.
        git.toplevel
            .insert(canonical_tmp.clone(), worktree_root.clone());
        // git_common_dir: checked for working_root ("/repo") and worktree_root.
        git.common_dir
            .insert(PathBuf::from("/repo"), common.clone());
        git.common_dir.insert(worktree_root.clone(), common.clone());

        let mut s = st("/repo");
        let saved_path_base = s.path_base.clone();
        let saved_working_root = s.working_root.clone();

        // Pass the temp dir as an absolute path → resolve_worktree_path returns it directly,
        // target.exists() is true → worktree_add is NOT called.
        let frame = enter(&mut s, &git, Some(canonical_tmp.clone()), None).unwrap();

        // Returned frame holds the PRE-change state.
        assert_eq!(frame.path_base, saved_path_base);
        assert_eq!(frame.working_root, saved_working_root);

        // Stack has exactly one entry (the saved frame).
        assert_eq!(s.stack.len(), 1);

        // State updated to the worktree.
        assert_eq!(s.path_base, canonical_tmp);
        assert_eq!(s.working_root, worktree_root);

        // worktree_add was NOT invoked.
        assert!(git.added.lock().unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
