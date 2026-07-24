use async_trait::async_trait;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct TokioCancellation(pub CancellationToken);
#[async_trait]
impl tools::CancellationSignal for TokioCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
    async fn cancelled(&self) {
        self.0.cancelled().await
    }
    fn child_signal(&self) -> Arc<dyn tools::CancellationSignal> {
        Arc::new(Self(self.0.child_token()))
    }
}
pub fn cancellation(token: CancellationToken) -> Arc<dyn tools::CancellationSignal> {
    Arc::new(TokioCancellation(token))
}

pub struct ChannelProgress(pub tokio::sync::mpsc::Sender<tools::AgentProgressEvent>);
impl tools::ProgressSink for ChannelProgress {
    fn emit(&self, event: tools::AgentProgressEvent) {
        let _ = self.0.try_send(event);
    }
}
pub fn progress(
    tx: tokio::sync::mpsc::Sender<tools::AgentProgressEvent>,
) -> Arc<dyn tools::ProgressSink> {
    Arc::new(ChannelProgress(tx))
}

/// Runtime-owned workspace capabilities. The tools context only receives the
/// read view; dispatch/control/persistence stay on explicit Runtime paths.
#[derive(Clone)]
pub struct RuntimeWorkspaceAccess(project::WorkspaceViews);
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

#[cfg(test)]
mod tests {
    #[test]
    fn sub_agent_workspace_isolated() {
        use project::WorkspaceError;

        let main_dir = tempfile::tempdir().unwrap();
        let child_dir = main_dir.path().join("child");
        std::fs::create_dir_all(&child_dir).unwrap();
        let parent = project::wire_production_workspace(main_dir.path().to_path_buf())
            .expect("workspace initialization")
            .into_views();
        parent
            .control()
            .change_directory(child_dir.clone())
            .expect("change parent directory");

        let child = parent.derive_isolated();
        let canonical_main = main_dir.path().canonicalize().unwrap();
        let canonical_child = child_dir.canonicalize().unwrap();
        assert_eq!(child.read().current_path_base(), canonical_child);
        assert_eq!(child.read().current_workspace_root(), canonical_main);

        child
            .control()
            .change_directory(main_dir.path().to_path_buf())
            .expect("change child directory");
        assert_eq!(child.read().current_path_base(), canonical_main);
        assert_eq!(parent.read().current_path_base(), canonical_child);
        assert_eq!(
            child.control().exit(),
            Err(WorkspaceError::UnsupportedForNonGit)
        );
    }
}
