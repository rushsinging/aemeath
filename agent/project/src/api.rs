pub use crate::worktree::{
    enter_worktree, exit_worktree, restore_workspace_context,
    workspace_context_from_tool_context,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectApiMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_is_copy() {
        let marker = ProjectApiMarker;
        assert_eq!(marker, marker);
    }
}
