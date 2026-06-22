#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceState {
    pub cwd: Option<String>,
    pub worktree: Option<String>,
    pub path_base: Option<String>,
    pub workspace_root: Option<String>,
    pub branch: Option<String>,
    pub kind: WorktreeKind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorktreeKind {
    #[default]
    Unknown,
    MainCheckout,
    LinkedWorktree,
}
