#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectApiMarker;

use share::session_types::PersistedWorkspaceContext;
use std::path::{Path, PathBuf};

/// Runtime worktree stack frame（替代 share::tool::WorkingContext）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFrame {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}

/// Workspace 层集中错误（用户可见消息为中文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    PathNotFound(PathBuf),
    MissingPathAndBranch,
    InvalidBranch,
    NestedWorktree,
    RepoMismatch { path: PathBuf, repo_root: PathBuf },
    EmptyStack,
    RestoreInvalidPath(PathBuf),
    Git(String),
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceError::PathNotFound(p) => write!(f, "路径不存在或无法访问 {}", p.display()),
            WorkspaceError::MissingPathAndBranch => {
                write!(f, "进入或创建 worktree 时必须提供 path 或 branch")
            }
            WorkspaceError::InvalidBranch => write!(f, "branch 不能只包含路径分隔符或敏感字符"),
            WorkspaceError::NestedWorktree => write!(
                f,
                "已在 worktree 中，请先 ExitWorktree 退出当前 worktree 再进入新的"
            ),
            WorkspaceError::RepoMismatch { path, repo_root } => write!(
                f,
                "路径 {} 不属于当前仓库（当前仓库根: {}）",
                path.display(),
                repo_root.display()
            ),
            WorkspaceError::EmptyStack => write!(
                f,
                "上下文栈为空，没有可恢复的 worktree。可能已经在主工作区。"
            ),
            WorkspaceError::RestoreInvalidPath(p) => {
                write!(f, "恢复工作区失败：路径不存在 {}", p.display())
            }
            WorkspaceError::Git(m) => write!(f, "{}", m),
        }
    }
}
impl std::error::Error for WorkspaceError {}

/// 读当前 workspace 位置（所有 tool 可用）。
pub trait WorkspaceRead: Send + Sync {
    fn current_root(&self) -> PathBuf;
    fn current_path_base(&self) -> PathBuf;
    fn resolve(&self, rel: &Path) -> PathBuf;
}

/// 运行期 workspace 变更（bash cd + worktree enter/exit）。
pub trait WorkspaceControl: Send + Sync {
    fn set_cwd(&self, path: PathBuf) -> Result<(), WorkspaceError>;
    fn enter(
        &self,
        path: Option<PathBuf>,
        branch: Option<String>,
    ) -> Result<WorkspaceFrame, WorkspaceError>;
    fn exit(&self) -> Result<WorkspaceFrame, WorkspaceError>;
}

/// session 边界持久化。
pub trait WorkspacePersist: Send + Sync {
    fn snapshot(&self) -> PersistedWorkspaceContext;
    fn restore(&self, dto: &PersistedWorkspaceContext) -> Result<(), WorkspaceError>;
}
