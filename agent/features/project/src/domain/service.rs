use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use share::session_types::PersistedWorkspaceContext;

use crate::domain::git::GitWorktreeOps;
use crate::domain::state::{self as rules, WorkspaceState};
use crate::domain::types::{
    WorkspaceControl, WorkspaceError, WorkspaceFrame, WorkspacePersist, WorkspaceRead,
};

/// project 拥有的唯一可变 workspace 状态源（单锁）。
pub(crate) struct WorkspaceService {
    state: Mutex<WorkspaceState>,
    control_operation: Mutex<()>,
    git: Arc<dyn GitWorktreeOps>,
}

impl WorkspaceService {
    pub(crate) fn with_git(cwd: PathBuf, git: Arc<dyn GitWorktreeOps>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(WorkspaceState::new(cwd)),
            control_operation: Mutex::new(()),
            git,
        })
    }
    /// 从当前快照派生独立实例（继承 root/base、空栈、新锁），供子 agent。
    pub fn seed_isolated(&self) -> Arc<Self> {
        let s = self.lock();
        Arc::new(Self {
            state: Mutex::new(WorkspaceState {
                initial_cwd: s.initial_cwd.clone(),
                workspace_root: s.workspace_root.clone(),
                path_base: s.path_base.clone(),
                stack: Vec::new(),
            }),
            control_operation: Mutex::new(()),
            git: self.git.clone(),
        })
    }
    fn lock(&self) -> MutexGuard<'_, WorkspaceState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_control(&self) -> MutexGuard<'_, ()> {
        self.control_operation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn candidate(&self) -> WorkspaceState {
        self.lock().clone()
    }

    fn commit(&self, candidate: WorkspaceState) {
        *self.lock() = candidate;
    }
}

impl WorkspaceRead for WorkspaceService {
    fn current_workspace_root(&self) -> PathBuf {
        self.lock().workspace_root.clone()
    }
    fn current_path_base(&self) -> PathBuf {
        self.lock().path_base.clone()
    }
    fn resolve(&self, rel: &Path) -> PathBuf {
        self.lock().resolve(rel)
    }
    fn in_worktree(&self) -> bool {
        // 先克隆 workspace_root 释放状态锁，避免持锁期间 spawn git 子进程。
        let root = self.lock().workspace_root.clone();
        self.git.in_worktree(&root).unwrap_or(false)
    }
    fn current_branch(&self) -> Result<Option<String>, WorkspaceError> {
        let root = self.lock().workspace_root.clone();
        self.git.current_branch(&root).map_err(WorkspaceError::Git)
    }
    fn initial_cwd(&self) -> PathBuf {
        self.lock().initial_cwd.clone()
    }
}

impl WorkspaceControl for WorkspaceService {
    fn set_path_base(&self, path: PathBuf) -> Result<(), WorkspaceError> {
        let _control = self.lock_control();
        let mut candidate = self.candidate();
        rules::set_path_base(&mut candidate, path)?;
        self.commit(candidate);
        Ok(())
    }

    fn set_workspace_root(&self, root: PathBuf, path: PathBuf) -> Result<(), WorkspaceError> {
        let _control = self.lock_control();
        let mut candidate = self.candidate();
        rules::set_workspace_root(&mut candidate, root, path)?;
        self.commit(candidate);
        Ok(())
    }
    fn switch_to(&self, path: PathBuf) -> Result<(), WorkspaceError> {
        let _control = self.lock_control();
        let mut candidate = self.candidate();
        rules::switch_to(&mut candidate, self.git.as_ref(), path)?;
        self.commit(candidate);
        Ok(())
    }
    fn enter(
        &self,
        path: Option<PathBuf>,
        branch: Option<String>,
    ) -> Result<WorkspaceFrame, WorkspaceError> {
        let _control = self.lock_control();
        let mut candidate = self.candidate();
        let frame = rules::enter(&mut candidate, self.git.as_ref(), path, branch)?;
        self.commit(candidate);
        Ok(frame)
    }
    fn exit(&self) -> Result<WorkspaceFrame, WorkspaceError> {
        let _control = self.lock_control();
        let mut candidate = self.candidate();
        let frame = rules::exit(&mut candidate)?;
        self.commit(candidate);
        Ok(frame)
    }
}

impl WorkspacePersist for WorkspaceService {
    fn snapshot(&self) -> PersistedWorkspaceContext {
        rules::snapshot(&self.lock())
    }
    fn restore(&self, dto: &PersistedWorkspaceContext) -> Result<(), WorkspaceError> {
        let _control = self.lock_control();
        let mut candidate = self.candidate();
        rules::restore(&mut candidate, dto)?;
        self.commit(candidate);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::git::tests::FakeGit;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use std::time::Duration;

    struct BlockingGit {
        target: PathBuf,
        worktree_root: PathBuf,
        common_dir: PathBuf,
        io_started: Sender<()>,
        io_release: Mutex<Receiver<()>>,
    }

    impl GitWorktreeOps for BlockingGit {
        fn git_common_dir(&self, _path: &Path) -> Result<PathBuf, String> {
            Ok(self.common_dir.clone())
        }

        fn show_toplevel(&self, path: &Path) -> Result<PathBuf, String> {
            assert_eq!(path, self.target);
            self.io_started.send(()).unwrap();
            self.io_release.lock().unwrap().recv().unwrap();
            Ok(self.worktree_root.clone())
        }

        fn in_worktree(&self, _path: &Path) -> Result<bool, String> {
            Ok(false)
        }

        fn worktree_add(
            &self,
            _repo_root: &Path,
            _path: &Path,
            _branch: &str,
            _base: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        fn current_branch(&self, _path: &Path) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "aemeath_project_{name}_{}_{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path.canonicalize().unwrap()
    }

    #[test]
    fn switch_during_git_io_keeps_committed_state_readable() {
        let root = unique_temp_dir("read_during_io_root");
        let target = unique_temp_dir("read_during_io_target");
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let git = Arc::new(BlockingGit {
            target: target.clone(),
            worktree_root: target.clone(),
            common_dir: root.join(".git"),
            io_started: started_tx,
            io_release: Mutex::new(release_rx),
        });
        let workspace = WorkspaceService::with_git(root.clone(), git);
        let switching = {
            let workspace = workspace.clone();
            let target = target.clone();
            thread::spawn(move || workspace.switch_to(target))
        };

        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let (read_tx, read_rx) = mpsc::channel();
        let reader = {
            let workspace = workspace.clone();
            thread::spawn(move || read_tx.send(workspace.current_path_base()).unwrap())
        };

        let observed = read_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("Git I/O 期间 state lock 不应阻塞只读访问");
        assert_eq!(observed, root);
        release_tx.send(()).unwrap();
        assert_eq!(switching.join().unwrap(), Ok(()));
        reader.join().unwrap();
        assert_eq!(workspace.current_path_base(), target);
    }

    #[test]
    fn concurrent_writes_share_one_control_operation_lock() {
        let root = unique_temp_dir("serialized_root");
        let target = unique_temp_dir("serialized_target");
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let git = Arc::new(BlockingGit {
            target: target.clone(),
            worktree_root: target.clone(),
            common_dir: root.join(".git"),
            io_started: started_tx,
            io_release: Mutex::new(release_rx),
        });
        let workspace = WorkspaceService::with_git(root.clone(), git);
        let first = {
            let workspace = workspace.clone();
            let target = target.clone();
            thread::spawn(move || workspace.switch_to(target))
        };
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let (second_done_tx, second_done_rx) = mpsc::channel();
        let second = {
            let workspace = workspace.clone();
            let root = root.clone();
            thread::spawn(move || {
                let result = workspace.set_path_base(root);
                second_done_tx.send(()).unwrap();
                result
            })
        };
        assert!(
            second_done_rx
                .recv_timeout(Duration::from_millis(200))
                .is_err(),
            "第二个写操作必须等待同一 control-operation mutex"
        );

        release_tx.send(()).unwrap();
        assert_eq!(first.join().unwrap(), Ok(()));
        second_done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(second.join().unwrap(), Ok(()));
        assert_eq!(workspace.current_path_base(), root);
    }

    #[test]
    fn seed_isolated_inherits_position_empty_stack_independent_lock() {
        let parent =
            WorkspaceService::with_git(PathBuf::from("/repo"), Arc::new(FakeGit::default()));
        // 父进入一个伪 worktree 帧
        {
            let mut s = parent.lock();
            s.path_base = "/wt".into();
            s.workspace_root = "/wt".into();
            s.stack.push(WorkspaceFrame {
                path_base: "/repo".into(),
                workspace_root: "/repo".into(),
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
