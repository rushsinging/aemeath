use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use share::session_types::WorkspaceContext;
use share::tool::WorkingContext;

pub use crate::business::working_paths::{current_path, new_working_paths, set_working_directory};
pub use crate::business::worktree::{
    enter_worktree, exit_worktree, restore_workspace_context,
    workspace_context_from_worktree_context, WorktreeWorkingContext,
};

/// OHS gateway for project paths, worktree transitions, and workspace context.
pub trait ProjectGateway: Send + Sync {
    fn new_working_paths(
        &self,
        cwd: PathBuf,
    ) -> (PathBuf, Arc<Mutex<PathBuf>>, Arc<Mutex<PathBuf>>);

    fn current_path(&self, path: &Arc<Mutex<PathBuf>>) -> PathBuf;

    fn set_working_directory(
        &self,
        working_root: &Arc<Mutex<PathBuf>>,
        path_base: &Arc<Mutex<PathBuf>>,
        path: PathBuf,
    );

    fn enter_worktree(
        &self,
        ctx: &WorktreeWorkingContext,
        path: Option<PathBuf>,
        branch: Option<String>,
    ) -> Result<WorkingContext, String>;

    fn exit_worktree(&self, ctx: &WorktreeWorkingContext) -> Result<WorkingContext, String>;

    fn workspace_context_from_worktree_context(
        &self,
        ctx: &WorktreeWorkingContext,
    ) -> WorkspaceContext;

    fn restore_workspace_context(
        &self,
        ctx: &WorktreeWorkingContext,
        workspace: &WorkspaceContext,
    ) -> Result<(), String>;
}

/// Default project gateway backed by the existing project path/worktree functions.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultProjectGateway;

pub fn wire_project() -> Arc<dyn ProjectGateway> {
    Arc::new(DefaultProjectGateway)
}

impl ProjectGateway for DefaultProjectGateway {
    fn new_working_paths(
        &self,
        cwd: PathBuf,
    ) -> (PathBuf, Arc<Mutex<PathBuf>>, Arc<Mutex<PathBuf>>) {
        new_working_paths(cwd)
    }

    fn current_path(&self, path: &Arc<Mutex<PathBuf>>) -> PathBuf {
        current_path(path)
    }

    fn set_working_directory(
        &self,
        working_root: &Arc<Mutex<PathBuf>>,
        path_base: &Arc<Mutex<PathBuf>>,
        path: PathBuf,
    ) {
        set_working_directory(working_root, path_base, path);
    }

    fn enter_worktree(
        &self,
        ctx: &WorktreeWorkingContext,
        path: Option<PathBuf>,
        branch: Option<String>,
    ) -> Result<WorkingContext, String> {
        enter_worktree(ctx, path, branch)
    }

    fn exit_worktree(&self, ctx: &WorktreeWorkingContext) -> Result<WorkingContext, String> {
        exit_worktree(ctx)
    }

    fn workspace_context_from_worktree_context(
        &self,
        ctx: &WorktreeWorkingContext,
    ) -> WorkspaceContext {
        workspace_context_from_worktree_context(ctx)
    }

    fn restore_workspace_context(
        &self,
        ctx: &WorktreeWorkingContext,
        workspace: &WorkspaceContext,
    ) -> Result<(), String> {
        restore_workspace_context(ctx, workspace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_project_gateway_is_object_safe_and_callable() {
        let gateway: &dyn ProjectGateway = &DefaultProjectGateway;
        let cwd = PathBuf::from("/tmp/aemeath");

        let (returned_cwd, working_root, path_base) = gateway.new_working_paths(cwd.clone());

        assert_eq!(returned_cwd, cwd);
        assert_eq!(gateway.current_path(&working_root), cwd);
        assert_eq!(
            gateway.current_path(&path_base),
            PathBuf::from("/tmp/aemeath")
        );
    }
}
