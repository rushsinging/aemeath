use std::path::PathBuf;
use std::sync::Arc;

use crate::adapters::git::GitCli;
use crate::domain::service::WorkspaceService;
use crate::domain::types::{WorkspaceControl, WorkspacePersist, WorkspaceRead};

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

pub fn wire_production_workspace(cwd: PathBuf) -> WorkspaceWiring {
    WorkspaceWiring {
        service: WorkspaceService::with_git(cwd, Arc::new(GitCli)),
    }
}
