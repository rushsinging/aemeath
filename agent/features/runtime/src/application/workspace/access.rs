//! Runtime-owned workspace capabilities.
//!
//! The tools context only receives the read view; dispatch/control/persistence
//! stay on explicit Runtime paths.

use std::sync::Arc;

/// Runtime-owned workspace capabilities.
#[derive(Clone)]
pub struct RuntimeWorkspaceAccess(pub project::WorkspaceViews);

impl RuntimeWorkspaceAccess {
    pub fn new(views: project::WorkspaceViews) -> Self {
        Self(views)
    }
    pub fn read_access(&self) -> tools::WorkspaceReadAccess {
        tools::WorkspaceReadAccess::new(self.0.read())
    }
    pub fn derive_isolated(&self) -> Self {
        Self(self.0.derive_isolated())
    }
    pub fn views(&self) -> project::WorkspaceViews {
        self.0.clone()
    }
    pub fn control(&self) -> Arc<dyn project::WorkspaceControl> {
        self.0.control()
    }
    pub fn persist(&self) -> Arc<dyn project::WorkspacePersist> {
        self.0.persist()
    }
}
