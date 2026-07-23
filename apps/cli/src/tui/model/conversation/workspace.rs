#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorktreeKind {
    #[default]
    Unknown,
    MainCheckout,
    LinkedWorktree,
}
