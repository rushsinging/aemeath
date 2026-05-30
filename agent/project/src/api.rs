pub use crate::business::working_paths::{current_path, new_working_paths, set_working_directory};
pub use crate::business::worktree::{
    enter_worktree, exit_worktree, restore_workspace_context, workspace_context_from_tool_context,
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
