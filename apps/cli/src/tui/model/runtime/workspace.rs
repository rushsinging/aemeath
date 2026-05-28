#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceState {
    pub cwd: Option<String>,
    pub worktree: Option<String>,
}
