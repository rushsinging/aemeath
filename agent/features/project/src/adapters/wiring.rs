use std::path::PathBuf;
use std::sync::Arc;

use crate::adapters::git::GitCli;
use crate::domain::git::{GitWorktreeOps, RepositoryProbe};
use crate::domain::service::WorkspaceService;
use crate::domain::types::{WorkspaceControl, WorkspaceInitError, WorkspacePersist, WorkspaceRead};
use share::session_types::{ProjectIdentity, WorktreeKind};

#[derive(Clone)]
pub struct WorkspaceWiring {
    service: Arc<WorkspaceService>,
}

impl WorkspaceWiring {
    pub fn read(&self) -> Arc<dyn WorkspaceRead> {
        self.service.clone()
    }

    pub fn control(&self) -> Arc<dyn WorkspaceControl> {
        self.service.clone()
    }

    pub fn persist(&self) -> Arc<dyn WorkspacePersist> {
        self.service.clone()
    }

    pub fn derive_isolated(&self) -> Self {
        Self {
            service: self.service.seed_isolated(),
        }
    }

    pub fn into_views(self) -> WorkspaceViews {
        WorkspaceViews {
            read: self.read(),
            control: self.control(),
            persist: self.persist(),
            derive_isolated: Arc::new(move || self.derive_isolated().into_views()),
        }
    }
}

#[derive(Clone)]
pub struct WorkspaceViews {
    read: Arc<dyn WorkspaceRead>,
    control: Arc<dyn WorkspaceControl>,
    persist: Arc<dyn WorkspacePersist>,
    derive_isolated: Arc<dyn Fn() -> WorkspaceViews + Send + Sync>,
}

impl WorkspaceViews {
    pub fn read(&self) -> Arc<dyn WorkspaceRead> {
        self.read.clone()
    }

    pub fn control(&self) -> Arc<dyn WorkspaceControl> {
        self.control.clone()
    }

    pub fn persist(&self) -> Arc<dyn WorkspacePersist> {
        self.persist.clone()
    }

    pub fn derive_isolated(&self) -> Self {
        (self.derive_isolated)()
    }
}

pub fn wire_production_workspace(cwd: PathBuf) -> Result<WorkspaceWiring, WorkspaceInitError> {
    log::info!(target: crate::LOG_TARGET, "wire_production_workspace enter");
    match build_workspace(cwd) {
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
fn build_workspace(cwd: PathBuf) -> Result<(WorkspaceWiring, WorktreeKind), WorkspaceInitError> {
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
            ProjectIdentity {
                initial_cwd: canonical.display().to_string(),
                git_common_dir: Some(canonical_common_dir.display().to_string()),
            },
            canonical_top_level,
            worktree_kind,
        ),
        RepositoryProbe::NonGit => (
            ProjectIdentity {
                initial_cwd: canonical.display().to_string(),
                git_common_dir: None,
            },
            canonical,
            share::session_types::WorktreeKind::NonGit,
        ),
    };
    Ok((
        WorkspaceWiring {
            service: WorkspaceService::with_verified_git(
                identity,
                workspace_root,
                canonical_path_base,
                kind,
                git,
            ),
        },
        kind,
    ))
}
