use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::adapters::git::GitCli;
use crate::domain::git::{GitWorktreeOps, RepositoryProbe};
use crate::domain::service::WorkspaceService;
use crate::domain::types::{
    WorkspaceControl, WorkspaceInitError, WorkspaceReader, WorkspaceWriter,
};
use share::session_types::{ProjectIdentityData, WorktreeKind};

/// workspace 域句柄角色：三窄面 accessor + 隔离派生。
#[derive(Clone)]
pub struct Workspace {
    read: Arc<dyn WorkspaceReader>,
    control: Arc<dyn WorkspaceControl>,
    persist: Arc<dyn WorkspaceWriter>,
    derive_isolated: Arc<dyn Fn() -> Workspace + Send + Sync>,
}

impl Workspace {
    pub fn read(&self) -> Arc<dyn WorkspaceReader> {
        self.read.clone()
    }

    pub fn control(&self) -> Arc<dyn WorkspaceControl> {
        self.control.clone()
    }

    pub fn persist(&self) -> Arc<dyn WorkspaceWriter> {
        self.persist.clone()
    }

    pub fn derive_isolated(&self) -> Self {
        (self.derive_isolated)()
    }
}

pub fn wire_production_workspace(
    cwd: PathBuf,
    worktrees_dir: Option<PathBuf>,
) -> Result<Workspace, WorkspaceInitError> {
    log::info!(target: crate::LOG_TARGET, "wire_production_workspace enter");
    match build_workspace(cwd, worktrees_dir) {
        Ok((wiring, kind)) => {
            log::info!(
                target: crate::LOG_TARGET,
                "wire_production_workspace success kind={kind:?}"
            );
            Ok(wiring)
        }
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "wire_production_workspace failure category={}",
                init_error_category(&error)
            );
            Err(error)
        }
    }
}

/// 稳定的错误类别名（仅 discriminant，不含路径等敏感信息），供安全日志使用。
fn init_error_category(error: &WorkspaceInitError) -> &'static str {
    match error {
        WorkspaceInitError::PathNotFound { .. } => "PathNotFound",
        WorkspaceInitError::NotDirectory { .. } => "NotDirectory",
        WorkspaceInitError::PermissionDenied { .. } => "PermissionDenied",
        WorkspaceInitError::CanonicalizeFailed { .. } => "CanonicalizeFailed",
        WorkspaceInitError::GitProbeFailed(_) => "GitProbeFailed",
    }
}

/// 不含日志副作用的构造逻辑；行为与错误类型与重构前完全一致。
fn build_workspace(
    cwd: PathBuf,
    worktrees_dir: Option<PathBuf>,
) -> Result<(Workspace, WorktreeKind), WorkspaceInitError> {
    let metadata = std::fs::metadata(&cwd).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => WorkspaceInitError::PathNotFound { path: cwd.clone() },
        std::io::ErrorKind::PermissionDenied => {
            WorkspaceInitError::PermissionDenied { path: cwd.clone() }
        }
        _ => WorkspaceInitError::CanonicalizeFailed { path: cwd.clone() },
    })?;
    if !metadata.is_dir() {
        return Err(WorkspaceInitError::NotDirectory { path: cwd });
    }
    let canonical = cwd.canonicalize().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => WorkspaceInitError::PathNotFound { path: cwd.clone() },
        std::io::ErrorKind::PermissionDenied => {
            WorkspaceInitError::PermissionDenied { path: cwd.clone() }
        }
        _ => WorkspaceInitError::CanonicalizeFailed { path: cwd.clone() },
    })?;
    let canonical_path_base = canonical.clone();
    let git: Arc<dyn GitWorktreeOps> = Arc::new(GitCli);
    let (identity, workspace_root, kind) = match git
        .probe_repository(&canonical)
        .map_err(WorkspaceInitError::GitProbeFailed)?
    {
        RepositoryProbe::Git {
            canonical_top_level,
            canonical_common_dir,
            worktree_kind,
        } => (
            ProjectIdentityData {
                initial_cwd: canonical.display().to_string(),
                git_common_dir: Some(canonical_common_dir.display().to_string()),
            },
            canonical_top_level,
            worktree_kind,
        ),
        RepositoryProbe::NonGit => (
            ProjectIdentityData {
                initial_cwd: canonical.display().to_string(),
                git_common_dir: None,
            },
            canonical,
            share::session_types::WorktreeKind::NonGit,
        ),
    };
    let worktrees_root = resolve_worktrees_root(worktrees_dir, &workspace_root);
    let service = WorkspaceService::with_verified_git(
        identity,
        workspace_root,
        canonical_path_base,
        kind,
        worktrees_root,
        git,
    );
    Ok((workspace_handle(service), kind))
}

/// 由底层 service 构造域句柄（隔离派生链经 seed_isolated 递归生成）。
fn workspace_handle(service: Arc<WorkspaceService>) -> Workspace {
    let derive_service = Arc::clone(&service);
    Workspace {
        read: service.clone(),
        control: service.clone(),
        persist: service,
        derive_isolated: Arc::new(move || workspace_handle(derive_service.seed_isolated())),
    }
}

/// 解析 worktree 默认根目录：未配置时用全局 `~/.agents/worktrees`；
/// 相对路径相对当前 workspace root（可配 `.worktrees` 恢复仓库内布局）；
/// 绝对路径原样生效。project 自身不读 env / config，只消费注入值。
fn resolve_worktrees_root(worktrees_dir: Option<PathBuf>, workspace_root: &Path) -> PathBuf {
    match worktrees_dir {
        Some(dir) if dir.is_absolute() => dir,
        Some(dir) => workspace_root.join(dir),
        None => share::config::paths::global_worktrees_dir(),
    }
}
