//! Workspace 领域类型与能力端口（domain-owned）。
//!
//! 这些类型由 `WorkspaceService` / `WorkspaceState` 实现与消费，并由 crate root
//! 精确 re-export 为 Project 的稳定 façade。

use share::session_types::{PersistedWorkspaceContext, ProjectIdentity, WorkspaceId, WorktreeKind};
use std::path::{Path, PathBuf};

/// Runtime worktree stack frame（替代 share::tool::WorkingContext）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFrame {
    pub path_base: PathBuf,
    pub workspace_root: PathBuf,
    pub worktree_kind: WorktreeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitProbeError {
    GitUnavailable,
    PermissionDenied,
    CommandFailed { exit_code: Option<i32> },
    InvalidOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitOperationError {
    GitUnavailable,
    PermissionDenied,
    CommandFailed { exit_code: Option<i32> },
    InvalidOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceInitError {
    PathNotFound { path: PathBuf },
    NotDirectory { path: PathBuf },
    PermissionDenied { path: PathBuf },
    CanonicalizeFailed { path: PathBuf },
    GitProbeFailed(GitProbeError),
}

impl std::fmt::Display for WorkspaceInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathNotFound { path } => write!(f, "路径不存在：{}", path.display()),
            Self::NotDirectory { path } => write!(f, "路径不是目录：{}", path.display()),
            Self::PermissionDenied { path } => write!(f, "无权访问路径：{}", path.display()),
            Self::CanonicalizeFailed { path } => {
                write!(f, "无法规范化路径：{}", path.display())
            }
            Self::GitProbeFailed(error) => write!(f, "Git 仓库探测失败：{error:?}"),
        }
    }
}

impl std::error::Error for WorkspaceInitError {}

/// Workspace 层集中错误（用户可见消息为中文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    PathNotFound(PathBuf),
    NotDirectory(PathBuf),
    PathOutsideWorkspaceRoot {
        path: PathBuf,
        root: PathBuf,
    },
    PathTooDeep(PathBuf),
    CannotResolveSearchPath(PathBuf),
    MissingPathAndBranch,
    InvalidBranch,
    NestedWorktree {
        current_workspace_root: PathBuf,
        current_path_base: PathBuf,
    },
    RepoMismatch {
        path: PathBuf,
        repo_root: PathBuf,
    },
    NotLinkedWorktree {
        path: PathBuf,
    },
    EmptyStack,
    UnsupportedForNonGit,
    GitProbeFailed(GitProbeError),
    GitOperationFailed(GitOperationError),
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceError::PathNotFound(p) => write!(f, "路径不存在或无法访问 {}", p.display()),
            WorkspaceError::NotDirectory(p) => write!(f, "路径不是目录 {}", p.display()),
            WorkspaceError::PathOutsideWorkspaceRoot { path, root } => write!(
                f,
                "路径 {} 位于当前工作区根 {} 之外",
                path.display(),
                root.display()
            ),
            WorkspaceError::PathTooDeep(path) => {
                write!(f, "路径层级超过安全限制 {}", path.display())
            }
            WorkspaceError::CannotResolveSearchPath(path) => {
                write!(f, "无法解析搜索路径 {}", path.display())
            }
            WorkspaceError::MissingPathAndBranch => {
                write!(f, "进入或创建 worktree 时必须提供 path 或 branch")
            }
            WorkspaceError::InvalidBranch => write!(f, "branch 不能只包含路径分隔符或敏感字符"),
            WorkspaceError::NestedWorktree {
                current_workspace_root,
                current_path_base,
            } => write!(
                f,
                "已在 worktree 中（当前 workspace_root: {}，path_base: {}）。\
                 如需进入新 worktree，请先 ExitWorktree 退出当前 worktree 再进入新的；\
                 如目标一致，可直接在当前 worktree 继续工作",
                current_workspace_root.display(),
                current_path_base.display()
            ),
            WorkspaceError::RepoMismatch { path, repo_root } => write!(
                f,
                "路径 {} 不属于当前仓库（当前仓库根: {}）",
                path.display(),
                repo_root.display()
            ),
            WorkspaceError::NotLinkedWorktree { path } => {
                write!(f, "路径 {} 不是 linked worktree", path.display())
            }
            WorkspaceError::EmptyStack => write!(
                f,
                "上下文栈为空，没有可恢复的 worktree。可能已经在主工作区。"
            ),
            WorkspaceError::UnsupportedForNonGit => write!(f, "非 Git 项目不支持 worktree 操作"),
            WorkspaceError::GitProbeFailed(error) => write!(f, "Git 仓库探测失败：{error:?}"),
            WorkspaceError::GitOperationFailed(error) => write!(f, "Git 操作失败：{error:?}"),
        }
    }
}
impl std::error::Error for WorkspaceError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRestoreError {
    InvalidProjectIdentity,
    PathNotFound { path: String },
    PathOutsideWorkspaceRoot { path: String, root: String },
    InvalidStackShape,
    RepositoryMismatch,
    WorkspaceIdMismatch,
    GitProbeFailed(GitProbeError),
}

impl std::fmt::Display for WorkspaceRestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProjectIdentity => write!(f, "恢复工作区失败：项目身份无效"),
            Self::PathNotFound { path } => write!(f, "恢复工作区失败：路径不存在 {path}"),
            Self::PathOutsideWorkspaceRoot { path, root } => {
                write!(f, "恢复工作区失败：路径 {path} 位于工作区根 {root} 之外")
            }
            Self::InvalidStackShape => write!(f, "恢复工作区失败：上下文栈形状无效"),
            Self::RepositoryMismatch => write!(f, "恢复工作区失败：仓库身份或类型不匹配"),
            Self::WorkspaceIdMismatch => write!(f, "恢复工作区失败：workspace ID 不匹配"),
            Self::GitProbeFailed(error) => write!(f, "恢复工作区失败：Git 仓库探测失败：{error:?}"),
        }
    }
}

impl std::error::Error for WorkspaceRestoreError {}

/// 读当前 workspace 位置（所有 tool 可用）。
pub trait WorkspaceRead: Send + Sync {
    fn workspace_id(&self) -> WorkspaceId;
    fn project_identity(&self) -> ProjectIdentity;
    fn current_workspace_root(&self) -> PathBuf;
    fn current_path_base(&self) -> PathBuf;
    fn resolve(&self, rel: &Path) -> PathBuf;
    /// 解析文件路径；默认限制在 workspace root，授权方可显式放行越界路径。
    fn resolve_file_path(&self, path: &Path) -> Result<PathBuf, WorkspaceError>;
    fn resolve_file_path_authorized(
        &self,
        path: &Path,
        allow_outside_workspace: bool,
    ) -> Result<PathBuf, WorkspaceError> {
        if allow_outside_workspace {
            Ok(self.resolve(path))
        } else {
            self.resolve_file_path(path)
        }
    }
    /// 解析已存在的搜索目录；默认限制在 workspace root。
    fn resolve_search_path(&self, path: &Path) -> Result<PathBuf, WorkspaceError>;
    fn resolve_search_path_authorized(
        &self,
        path: &Path,
        allow_outside_workspace: bool,
    ) -> Result<PathBuf, WorkspaceError> {
        if !allow_outside_workspace {
            return self.resolve_search_path(path);
        }
        let resolved = self
            .resolve(path)
            .canonicalize()
            .map_err(|_| WorkspaceError::CannotResolveSearchPath(path.to_path_buf()))?;
        if !resolved.is_dir() {
            return Err(WorkspaceError::NotDirectory(resolved));
        }
        Ok(resolved)
    }
    /// 当前工作根是否位于 linked git worktree（`.git/worktrees/*`）。
    /// 用于 worktree 嵌套校验，防止在 worktree 内再创建 worktree。
    fn in_worktree(&self) -> bool;
    /// 当前分支名。detached HEAD / 无分支时返回 `Ok(None)`。
    fn current_branch(&self) -> Result<Option<String>, WorkspaceError>;
    /// 项目启动时的 cwd（init root），worktree 切换时**不变**。
    /// memory 等需要绑定项目身份（而非工作目录）的读写必须用此路径。
    fn initial_cwd(&self) -> PathBuf;
}

/// 运行期 workspace 变更（bash cd + worktree enter/exit）。
pub trait WorkspaceControl: Send + Sync {
    fn change_directory(&self, path: PathBuf) -> Result<(), WorkspaceError>;
    /// 切换到 `path`（存在性 + 同源校验），不压栈帧。供 ExitWorktree{path} 使用。
    fn switch_to(&self, path: PathBuf) -> Result<(), WorkspaceError>;
    fn enter(
        &self,
        path: Option<PathBuf>,
        branch: Option<String>,
        base: Option<String>,
    ) -> Result<WorkspaceFrame, WorkspaceError>;
    fn exit(&self) -> Result<WorkspaceFrame, WorkspaceError>;
}

/// session 边界持久化。
pub trait WorkspacePersist: Send + Sync {
    fn snapshot(&self) -> PersistedWorkspaceContext;
    fn prepare_restore(
        &self,
        dto: &PersistedWorkspaceContext,
    ) -> Result<crate::PreparedWorkspaceRestore, WorkspaceRestoreError>;
    fn commit_restore(&self, prepared: crate::PreparedWorkspaceRestore);
}
