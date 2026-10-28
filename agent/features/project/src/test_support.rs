//! Project 测试实现面（#2146 实现权治理：WorkspaceReader / WorkspaceControl /
//! WorkspaceWriter 实现权归本 crate，跨 crate 测试经此获得替身；
//! 生产不编译）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::WorkspaceControl;
use crate::WorkspaceData;
use crate::WorkspaceReader;
use share::session_types::ProjectIdentityData;
use share::session_types::WorkspaceId;

/// 纯内存工作区替身：路径解析 + 越界校验 + 可变 current 目录 +
/// `change_directory`（`enter`/`exit` 恒拒——NonGit 语义）。
#[derive(Clone)]
pub struct FakeWorkspace {
    pub(crate) initial_root: PathBuf,
    pub(crate) current: Arc<Mutex<PathBuf>>,
    pub(crate) enter_calls: Arc<Mutex<Vec<(Option<PathBuf>, Option<String>, Option<String>)>>>,
}

impl FakeWorkspace {
    pub fn new(root: PathBuf) -> Self {
        let root = root.canonicalize().unwrap_or(root);
        Self {
            initial_root: root.clone(),
            current: Arc::new(Mutex::new(root)),
            enter_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 记录到的 enter 调用参数（每次一行）。
    pub fn enter_calls(&self) -> Vec<(Option<PathBuf>, Option<String>, Option<String>)> {
        self.enter_calls
            .lock()
            .expect("fake workspace lock")
            .clone()
    }

    pub fn current(&self) -> PathBuf {
        self.current.lock().expect("fake workspace lock").clone()
    }

    fn normalized(&self, path: &Path) -> PathBuf {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.current().join(path)
        };
        candidate.canonicalize().unwrap_or(candidate)
    }
}

impl WorkspaceReader for FakeWorkspace {
    fn workspace_id(&self) -> WorkspaceId {
        WorkspaceId::new(format!("test-workspace:{}", self.initial_root.display()))
    }
    fn project_identity(&self) -> ProjectIdentityData {
        ProjectIdentityData {
            initial_cwd: self.initial_root.display().to_string(),
            git_common_dir: None,
        }
    }
    fn current_workspace_root(&self) -> PathBuf {
        self.initial_root.clone()
    }
    fn current_path_base(&self) -> PathBuf {
        self.current()
    }
    fn resolve(&self, rel: &Path) -> PathBuf {
        self.normalized(rel)
    }
    fn resolve_file_path(
        &self,
        path: &Path,
        auth: share::tools_vocab::AuthorizationContext,
    ) -> Result<PathBuf, share::error::DomainError> {
        let resolved = self.normalized(path);
        if auth.allow_outside_workspace || resolved.starts_with(&self.initial_root) {
            Ok(resolved)
        } else {
            Err(share::error::DomainError::invalid(
                "tools",
                format!("路径 {} 超出工作区根", resolved.display()),
            ))
        }
    }
    fn resolve_search_path(
        &self,
        path: &Path,
        auth: share::tools_vocab::AuthorizationContext,
    ) -> Result<PathBuf, share::error::DomainError> {
        let resolved = self.resolve_file_path(path, auth)?;
        if !resolved.exists() {
            return Err(share::error::DomainError::invalid(
                "tools",
                format!("路径不存在 {}", resolved.display()),
            ));
        }
        if !resolved.is_dir() {
            return Err(share::error::DomainError::invalid(
                "tools",
                format!("路径不是目录 {}", resolved.display()),
            ));
        }
        Ok(resolved)
    }
    fn in_worktree(&self) -> bool {
        false
    }
    fn current_branch(&self) -> Result<Option<String>, share::error::DomainError> {
        Ok(None)
    }
    fn initial_cwd(&self) -> PathBuf {
        self.initial_root.clone()
    }
}

impl WorkspaceControl for FakeWorkspace {
    fn change_directory(
        &self,
        path: PathBuf,
        _auth: share::tools_vocab::AuthorizationContext,
    ) -> Result<(), share::error::DomainError> {
        let resolved = self.normalized(&path);
        if !resolved.exists() {
            return Err(share::error::DomainError::invalid(
                "tools",
                format!("路径不存在 {}", resolved.display()),
            ));
        }
        if !resolved.is_dir() {
            return Err(share::error::DomainError::invalid(
                "tools",
                format!("路径不是目录 {}", resolved.display()),
            ));
        }
        *self.current.lock().expect("fake workspace lock") = resolved;
        Ok(())
    }
    fn enter(
        &self,
        path: Option<PathBuf>,
        branch: Option<String>,
        base: Option<String>,
    ) -> Result<WorkspaceData, share::error::DomainError> {
        self.enter_calls
            .lock()
            .expect("fake workspace lock")
            .push((path, branch, base));
        Err(share::error::DomainError::invalid(
            "tools",
            "NonGit 环境不支持 worktree 操作",
        ))
    }
    fn exit(&self) -> Result<WorkspaceData, share::error::DomainError> {
        Err(share::error::DomainError::invalid(
            "tools",
            "NonGit 环境不支持 worktree 操作",
        ))
    }
}

/// 固定快照的 WorkspaceWriter 替身：snapshot 返回构造值，
/// restore 通道 panic（未用场景）。
pub struct FixedWorkspaceWriter(pub share::session_types::PersistedWorkspaceContext);

impl crate::WorkspaceWriter for FixedWorkspaceWriter {
    fn snapshot(&self) -> share::session_types::PersistedWorkspaceContext {
        self.0.clone()
    }
    fn prepare_restore(
        &self,
        _dto: &share::session_types::PersistedWorkspaceContext,
    ) -> Result<crate::domain::state::WorkspaceRestoreData, share::error::DomainError> {
        panic!("not used")
    }
    fn commit_restore(&self, _prepared: crate::domain::state::WorkspaceRestoreData) {
        panic!("not used")
    }
}

// #2146 密封解锁（测试替身）。
impl crate::sealed::Sealed for FakeWorkspace {}
impl crate::sealed::Sealed for FixedWorkspaceWriter {}
