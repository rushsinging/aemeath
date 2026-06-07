use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use share::session_types::PersistedWorkspaceContext;

use crate::business::git_ops::{GitCli, GitWorktreeOps};
use crate::business::workspace_state::{self as rules, WorkspaceState};
use crate::contract::{
    WorkspaceControl, WorkspaceError, WorkspaceFrame, WorkspacePersist, WorkspaceRead,
};

/// project 拥有的唯一可变 workspace 状态源（单锁）。
pub struct WorkspaceService {
    state: Mutex<WorkspaceState>,
    git: Arc<dyn GitWorktreeOps>,
}

impl WorkspaceService {
    pub fn new(cwd: PathBuf) -> Arc<Self> {
        Self::with_git(cwd, Arc::new(GitCli))
    }
    pub fn with_git(cwd: PathBuf, git: Arc<dyn GitWorktreeOps>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(WorkspaceState::new(cwd)),
            git,
        })
    }
    /// 从当前快照派生独立实例（继承 root/base、空栈、新锁），供子 agent。
    pub fn seed_isolated(&self) -> Arc<Self> {
        let s = self.lock();
        Arc::new(Self {
            state: Mutex::new(WorkspaceState {
                initial_cwd: s.initial_cwd.clone(),
                working_root: s.working_root.clone(),
                path_base: s.path_base.clone(),
                stack: Vec::new(),
            }),
            git: self.git.clone(),
        })
    }
    fn lock(&self) -> MutexGuard<'_, WorkspaceState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl WorkspaceRead for WorkspaceService {
    fn current_root(&self) -> PathBuf {
        self.lock().working_root.clone()
    }
    fn current_path_base(&self) -> PathBuf {
        self.lock().path_base.clone()
    }
    fn resolve(&self, rel: &Path) -> PathBuf {
        self.lock().resolve(rel)
    }
}

impl WorkspaceControl for WorkspaceService {
    fn set_cwd(&self, path: PathBuf) -> Result<(), WorkspaceError> {
        rules::set_cwd(&mut self.lock(), self.git.as_ref(), path)
    }
    fn switch_to(&self, path: PathBuf) -> Result<(), WorkspaceError> {
        rules::switch_to(&mut self.lock(), self.git.as_ref(), path)
    }
    fn enter(
        &self,
        path: Option<PathBuf>,
        branch: Option<String>,
    ) -> Result<WorkspaceFrame, WorkspaceError> {
        rules::enter(&mut self.lock(), self.git.as_ref(), path, branch)
    }
    fn exit(&self) -> Result<WorkspaceFrame, WorkspaceError> {
        rules::exit(&mut self.lock())
    }
}

impl WorkspacePersist for WorkspaceService {
    fn snapshot(&self) -> PersistedWorkspaceContext {
        rules::snapshot(&self.lock())
    }
    fn restore(&self, dto: &PersistedWorkspaceContext) -> Result<(), WorkspaceError> {
        rules::restore(&mut self.lock(), dto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::git_ops::tests::FakeGit;

    #[test]
    fn seed_isolated_inherits_position_empty_stack_independent_lock() {
        let parent =
            WorkspaceService::with_git(PathBuf::from("/repo"), Arc::new(FakeGit::default()));
        // 父进入一个伪 worktree 帧
        {
            let mut s = parent.lock();
            s.path_base = "/wt".into();
            s.working_root = "/wt".into();
            s.stack.push(WorkspaceFrame {
                path_base: "/repo".into(),
                working_root: "/repo".into(),
            });
        }
        let child = parent.seed_isolated();
        assert_eq!(child.current_path_base(), PathBuf::from("/wt")); // 继承当前
                                                                     // 子退栈应为空（独立空栈）
        assert_eq!(
            WorkspaceControl::exit(child.as_ref()),
            Err(WorkspaceError::EmptyStack)
        );
        // 父仍有一帧（不受子影响）
        assert_eq!(parent.lock().stack.len(), 1);
    }
}
