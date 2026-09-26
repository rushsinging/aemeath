use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use share::session_types::{
    PersistedWorkspaceContext, ProjectIdentityData, WorkspaceId, WorktreeKind,
};

use crate::domain::git::GitWorktreeOps;
use crate::domain::state::WorkspaceRestoreData;
use crate::domain::state::{self as rules, WorkspaceState};
use crate::domain::types::{
    WorkspaceControl, WorkspaceData, WorkspaceError, WorkspaceReader, WorkspaceWriter,
};

const MAX_PATH_DEPTH: usize = 64;

fn resolve_path(
    path: &Path,
    path_base: &Path,
    workspace_root: Option<&Path>,
    must_exist: bool,
) -> Result<PathBuf, share::error::DomainError> {
    if path.components().count() > MAX_PATH_DEPTH {
        return Err(share::error::DomainError::from(
            WorkspaceError::PathTooDeep(path.to_path_buf()),
        ));
    }

    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        path_base.join(path)
    };
    let lexical = lexical_normalize(&joined);
    let resolved = if must_exist {
        lexical
            .canonicalize()
            .map_err(|_| WorkspaceError::CannotResolveSearchPath(path.to_path_buf()))?
    } else {
        canonicalize_existing_ancestor(&lexical)
    };
    if let Some(workspace_root) = workspace_root {
        let workspace = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| lexical_normalize(workspace_root));
        if !resolved.starts_with(&workspace) {
            return Err(share::error::DomainError::from(
                WorkspaceError::PathOutsideWorkspaceRoot {
                    path: resolved,
                    root: workspace,
                },
            ));
        }
    }
    Ok(resolved)
}

fn canonicalize_existing_ancestor(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    let mut tail = Vec::new();
    let mut current = Some(path);
    while let Some(candidate) = current {
        match candidate.canonicalize() {
            Ok(mut canonical) => {
                for component in tail.iter().rev() {
                    canonical.push(component);
                }
                return canonical;
            }
            Err(_) => {
                if let Some(name) = candidate.file_name() {
                    tail.push(name.to_os_string());
                }
                current = candidate.parent();
            }
        }
    }
    path.to_path_buf()
}

fn lexical_normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut stack = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(stack.last(), Some(Component::Normal(_))) {
                    stack.pop();
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                stack.clear();
                stack.push(component);
            }
            Component::Normal(_) => stack.push(component),
        }
    }
    stack
        .iter()
        .map(|component| component.as_os_str())
        .collect()
}

/// project 拥有的唯一可变 workspace 状态源（单锁）。
pub(crate) struct WorkspaceService {
    state: Mutex<WorkspaceState>,
    control_operation: Mutex<()>,
    git: Arc<dyn GitWorktreeOps>,
}

impl WorkspaceService {
    pub(crate) fn with_verified_git(
        project_identity: ProjectIdentityData,
        workspace_root: PathBuf,
        path_base: PathBuf,
        worktree_kind: WorktreeKind,
        worktrees_root: PathBuf,
        git: Arc<dyn GitWorktreeOps>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(WorkspaceState::from_verified(
                project_identity,
                workspace_root,
                path_base,
                worktree_kind,
                worktrees_root,
            )),
            control_operation: Mutex::new(()),
            git,
        })
    }
    #[cfg(test)]
    pub(crate) fn with_git(cwd: PathBuf, git: Arc<dyn GitWorktreeOps>) -> Arc<Self> {
        Self::with_verified_git(
            ProjectIdentityData {
                initial_cwd: cwd.display().to_string(),
                git_common_dir: Some(cwd.join(".git").display().to_string()),
            },
            cwd.clone(),
            cwd.clone(),
            WorktreeKind::Primary,
            cwd.join(".worktrees"),
            git,
        )
    }

    /// 从当前快照派生独立实例（继承 root/base、空栈、新锁），供子 agent。
    pub fn seed_isolated(&self) -> Arc<Self> {
        let s = self.lock();
        Arc::new(Self {
            state: Mutex::new(WorkspaceState {
                project_identity: s.project_identity.clone(),
                workspace_root: s.workspace_root.clone(),
                path_base: s.path_base.clone(),
                worktree_kind: s.worktree_kind,
                worktrees_root: s.worktrees_root.clone(),
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

impl WorkspaceReader for WorkspaceService {
    fn workspace_id(&self) -> WorkspaceId {
        self.lock().workspace_id()
    }
    fn project_identity(&self) -> ProjectIdentityData {
        self.lock().project_identity.clone()
    }
    fn current_workspace_root(&self) -> PathBuf {
        self.lock().workspace_root.clone()
    }
    fn current_path_base(&self) -> PathBuf {
        self.lock().path_base.clone()
    }
    fn resolve(&self, rel: &Path) -> PathBuf {
        self.lock().resolve(rel)
    }
    fn resolve_file_path(&self, path: &Path) -> Result<PathBuf, share::error::DomainError> {
        let state = self.lock();
        resolve_path(path, &state.path_base, Some(&state.workspace_root), false)
    }
    fn resolve_file_path_authorized(
        &self,
        path: &Path,
        allow_outside_workspace: bool,
    ) -> Result<PathBuf, share::error::DomainError> {
        let state = self.lock();
        resolve_path(
            path,
            &state.path_base,
            (!allow_outside_workspace).then_some(state.workspace_root.as_path()),
            false,
        )
    }
    fn resolve_search_path(&self, path: &Path) -> Result<PathBuf, share::error::DomainError> {
        let state = self.lock();
        resolve_path(path, &state.path_base, Some(&state.workspace_root), true)
    }
    fn resolve_search_path_authorized(
        &self,
        path: &Path,
        allow_outside_workspace: bool,
    ) -> Result<PathBuf, share::error::DomainError> {
        let state = self.lock();
        let resolved = resolve_path(
            path,
            &state.path_base,
            (!allow_outside_workspace).then_some(state.workspace_root.as_path()),
            true,
        )?;
        if !resolved.is_dir() {
            return Err(share::error::DomainError::from(
                WorkspaceError::NotDirectory(resolved),
            ));
        }
        Ok(resolved)
    }
    fn in_worktree(&self) -> bool {
        self.lock().worktree_kind == WorktreeKind::Linked
    }
    fn current_branch(&self) -> Result<Option<String>, share::error::DomainError> {
        let state = self.lock();
        if state.worktree_kind == WorktreeKind::NonGit {
            return Ok(None);
        }
        let root = state.workspace_root.clone();
        drop(state);
        self.git
            .current_branch(&root)
            .map_err(WorkspaceError::GitOperationFailed)
            .map_err(Into::into)
    }
    fn initial_cwd(&self) -> PathBuf {
        PathBuf::from(&self.lock().project_identity.initial_cwd)
    }
}

impl WorkspaceControl for WorkspaceService {
    fn change_directory(&self, path: PathBuf) -> Result<(), share::error::DomainError> {
        let _control = self.lock_control();
        let mut candidate = self.candidate();
        rules::change_directory(&mut candidate, path)?;
        self.commit(candidate);
        Ok(())
    }
    fn enter(
        &self,
        path: Option<PathBuf>,
        branch: Option<String>,
        base: Option<String>,
    ) -> Result<WorkspaceData, share::error::DomainError> {
        let _control = self.lock_control();
        let mut candidate = self.candidate();
        let frame = rules::enter(&mut candidate, self.git.as_ref(), path, branch, base)?;
        self.commit(candidate);
        Ok(frame)
    }
    fn exit(&self) -> Result<WorkspaceData, share::error::DomainError> {
        let _control = self.lock_control();
        let mut candidate = self.candidate();
        let frame = rules::exit(&mut candidate, self.git.as_ref())?;
        self.commit(candidate);
        Ok(frame)
    }
}

impl WorkspaceWriter for WorkspaceService {
    fn snapshot(&self) -> PersistedWorkspaceContext {
        rules::snapshot(&self.lock())
    }

    fn prepare_restore(
        &self,
        dto: &PersistedWorkspaceContext,
    ) -> Result<WorkspaceRestoreData, share::error::DomainError> {
        let live = self.candidate();
        rules::prepare_restore(&live, dto, self.git.as_ref()).map_err(Into::into)
    }

    fn commit_restore(&self, prepared: WorkspaceRestoreData) {
        let _control = self.lock_control();
        rules::commit_restore(&mut self.lock(), prepared);
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
        fn probe_repository(
            &self,
            path: &Path,
        ) -> Result<crate::domain::git::RepositoryProbe, crate::domain::types::GitProbeError>
        {
            // enter() 要求目标是 linked worktree，替身固定报告 Linked。
            Ok(crate::domain::git::RepositoryProbe::Git {
                canonical_top_level: path.to_path_buf(),
                canonical_common_dir: self.common_dir.clone(),
                worktree_kind: WorktreeKind::Linked,
            })
        }

        fn show_toplevel(
            &self,
            path: &Path,
        ) -> Result<PathBuf, crate::domain::types::GitOperationError> {
            assert_eq!(path, self.target);
            self.io_started.send(()).unwrap();
            self.io_release.lock().unwrap().recv().unwrap();
            Ok(self.worktree_root.clone())
        }

        fn is_linked_worktree(
            &self,
            _path: &Path,
        ) -> Result<bool, crate::domain::types::GitOperationError> {
            Ok(false)
        }

        fn worktree_add(
            &self,
            _repo_root: &Path,
            _path: &Path,
            _branch: &str,
            _base: &str,
        ) -> Result<(), crate::domain::types::GitOperationError> {
            Ok(())
        }

        fn current_branch(
            &self,
            _path: &Path,
        ) -> Result<Option<String>, crate::domain::types::GitOperationError> {
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
    fn resolve_file_path_allows_path_outside_workspace_when_authorized() {
        let root = unique_temp_dir("resolve_authorized_root");
        let outside = unique_temp_dir("resolve_authorized_outside").join("outside.rs");
        let service = WorkspaceService::with_git(root, Arc::new(FakeGit::default()));

        let result = service.resolve_file_path_authorized(&outside, true);

        assert_eq!(result.unwrap(), outside);
    }

    #[test]
    fn resolve_file_path_rejects_path_outside_workspace() {
        let root = unique_temp_dir("resolve_outside_root");
        let service = WorkspaceService::with_git(root.clone(), Arc::new(FakeGit::default()));

        let result = service.resolve_file_path(Path::new("../outside.rs"));

        let error = result.unwrap_err();
        assert!(
            error.message().starts_with("路径 "),
            "unexpected: {}",
            error.message()
        );
    }

    #[test]
    fn resolve_file_path_allows_missing_parents_inside_workspace() {
        let root = unique_temp_dir("resolve_missing_parent_root");
        let service = WorkspaceService::with_git(root.clone(), Arc::new(FakeGit::default()));

        let path = service
            .resolve_file_path(Path::new("deep/missing/new.rs"))
            .unwrap();

        assert_eq!(path, root.join("deep/missing/new.rs"));
    }

    #[test]
    fn resolve_search_path_rejects_path_outside_workspace() {
        let root = unique_temp_dir("search_outside_root");
        let outside = unique_temp_dir("search_outside_target");
        let service = WorkspaceService::with_git(root, Arc::new(FakeGit::default()));

        let result = service.resolve_search_path(&outside);

        let error = result.unwrap_err();
        assert!(
            error.message().starts_with("路径 "),
            "unexpected: {}",
            error.message()
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_file_path_rejects_symlink_escape() {
        let root = unique_temp_dir("resolve_symlink_root");
        let outside = unique_temp_dir("resolve_symlink_target");
        let link = root.join("escape");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let service = WorkspaceService::with_git(root, Arc::new(FakeGit::default()));

        let result = service.resolve_file_path(Path::new("escape/file.rs"));

        let error = result.unwrap_err();
        assert!(
            error.message().starts_with("路径 "),
            "unexpected: {}",
            error.message()
        );
    }

    #[test]
    fn enter_during_git_io_keeps_committed_state_readable() {
        let root = unique_temp_dir("read_during_io_root");
        let target = unique_temp_dir("read_during_io_target");
        std::fs::create_dir_all(&target).unwrap();
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
            thread::spawn(move || workspace.enter(Some(target), Some("branch".to_string()), None))
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
        assert!(switching.join().unwrap().is_ok());
        reader.join().unwrap();
        assert_eq!(workspace.current_path_base(), target);
    }

    #[test]
    fn concurrent_writes_share_one_control_operation_lock() {
        let root = unique_temp_dir("serialized_root");
        let target = unique_temp_dir("serialized_target");
        std::fs::create_dir_all(&target).unwrap();
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
            thread::spawn(move || workspace.enter(Some(target), Some("branch".to_string()), None))
        };
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        let (second_done_tx, second_done_rx) = mpsc::channel();
        let second = {
            let workspace = workspace.clone();
            let target = target.clone();
            thread::spawn(move || {
                let result = workspace.change_directory(target);
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
        assert!(first.join().unwrap().is_ok());
        second_done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(second.join().unwrap(), Ok(()));
        assert_eq!(workspace.current_path_base(), target);
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
            s.stack.push(WorkspaceData {
                id: WorkspaceId::new("test-frame"),
                path_base: "/repo".into(),
                workspace_root: "/repo".into(),
                worktree_kind: WorktreeKind::Primary,
            });
        }
        let child = parent.seed_isolated();
        assert_eq!(child.current_path_base(), PathBuf::from("/wt")); // 继承当前
                                                                     // 子退栈应为空（独立空栈）
        let exit_result = WorkspaceControl::exit(child.as_ref());
        assert!(matches!(
            exit_result,
            Err(error) if error.category() == share::error::ErrorCategory::Invalid
        ));
        // 父仍有一帧（不受子影响）
        assert_eq!(parent.lock().stack.len(), 1);
    }

    // ---- #894: WorkspaceWriter prepare_restore / commit_restore 令牌协议 ----

    /// 构造一个位于真实 temp root 的 git service，并配置 FakeGit 使 probe 自洽。
    fn git_service_at(root: &Path, common: &str) -> Arc<WorkspaceService> {
        let identity = ProjectIdentityData {
            initial_cwd: root.display().to_string(),
            git_common_dir: Some(common.to_string()),
        };
        let mut git = FakeGit::default();
        git.common_dir
            .insert(root.to_path_buf(), PathBuf::from(common));
        git.toplevel.insert(root.to_path_buf(), root.to_path_buf());
        WorkspaceService::with_verified_git(
            identity,
            root.to_path_buf(),
            root.to_path_buf(),
            WorktreeKind::Primary,
            root.join(".worktrees"),
            Arc::new(git),
        )
    }

    /// 组装一份自洽、真实存在、可通过 prepare 校验的 DTO（path_base 位于 root 内子目录）。
    fn valid_dto_with_subdir(root: &Path, common: &str) -> (PersistedWorkspaceContext, PathBuf) {
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let sub = sub.canonicalize().unwrap();
        let identity = ProjectIdentityData {
            initial_cwd: root.display().to_string(),
            git_common_dir: Some(common.to_string()),
        };
        let dto = PersistedWorkspaceContext {
            workspace_id: WorkspaceId::derive(&identity, &root.display().to_string()),
            project_identity: identity,
            path_base: sub.display().to_string(),
            workspace_root: root.display().to_string(),
            worktree_kind: WorktreeKind::Primary,
            context_stack: vec![],
        };
        (dto, sub)
    }

    #[test]
    fn prepare_restore_via_port_exposes_identity_and_leaves_live_state() {
        let root = unique_temp_dir("port_prepare_root");
        let common = "/repo/.git";
        let service = git_service_at(&root, common);
        let (dto, _sub) = valid_dto_with_subdir(&root, common);

        let observed_before = service.current_path_base();
        let prepared = service.prepare_restore(&dto).expect("合法 DTO 应构造令牌");

        // opaque token 唯一只读 accessor 暴露 Project 已校验 identity。
        assert_eq!(prepared.project_identity(), &dto.project_identity);
        // prepare NEVER 修改 live state：commit 前 observable 位置不变。
        assert_eq!(service.current_path_base(), observed_before);
        assert_eq!(service.current_path_base(), root);
    }

    #[test]
    fn commit_restore_via_port_returns_unit_and_replaces_state() {
        let root = unique_temp_dir("port_commit_root");
        let common = "/repo/.git";
        let service = git_service_at(&root, common);
        let (dto, sub) = valid_dto_with_subdir(&root, common);

        let prepared = service.prepare_restore(&dto).expect("合法 DTO 应构造令牌");

        // 签名 MUST 无 Result：一次性按值消费令牌、返回 unit。
        let unit: () = service.commit_restore(prepared);
        assert_eq!(unit, ());

        // 全量替换后可观测新 path_base。
        assert_eq!(service.current_path_base(), sub);
        assert_eq!(service.current_workspace_root(), root);
        assert_eq!(service.project_identity(), dto.project_identity);
    }

    #[test]
    fn prepare_restore_via_port_rejects_missing_path_and_keeps_live_state() {
        // 身份路径（workspace_root）缺失保持 PathNotFound；path_base 缺失已按
        // cwd 语义回退 workspace_root（见 state_tests 的 fallback 用例）。
        let root = unique_temp_dir("port_missing_root");
        let common = "/repo/.git";
        let service = git_service_at(&root, common);
        let mut dto = valid_dto_with_subdir(&root, common).0;
        dto.workspace_root = root.join("nope_missing").display().to_string();
        dto.path_base = valid_dto_with_subdir(&root, common).1.display().to_string();

        let before = service.current_path_base();
        let result = service.prepare_restore(&dto);

        assert!(
            result
                .as_ref()
                .err()
                .is_some_and(|error| error.message().contains("路径不存在")),
            "expected PathNotFound, got {result:?}"
        );
        assert_eq!(service.current_path_base(), before);
    }
}
