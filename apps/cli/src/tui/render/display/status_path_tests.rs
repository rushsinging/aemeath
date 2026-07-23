use crate::tui::app::event::UiEvent;
use crate::tui::app::{display_status_path, display_working_dir, status_context_for_paths};
use std::path::PathBuf;

#[test]
fn test_display_status_path_returns_absolute_for_non_home_path() {
    let path = PathBuf::from("/tmp/aemeath-status-line");

    let display = display_status_path(&path);

    assert!(display.starts_with('/'));
    assert_eq!(display, "/tmp/aemeath-status-line");
}

#[test]
fn test_display_working_dir_still_returns_leaf_name() {
    let path = PathBuf::from("/tmp/aemeath-status-line");

    let display = display_working_dir(&path);

    assert_eq!(display, "aemeath-status-line");
}

#[test]
fn test_working_directory_changed_carries_full_status_context() {
    let path_base = PathBuf::from("/tmp/aemeath-status-line/subdir");
    let workspace_root = PathBuf::from("/tmp/aemeath-status-line");

    let event = status_context_for_paths(&path_base, &workspace_root);

    match event {
        UiEvent::WorkingDirectoryChanged(ctx) => {
            assert_eq!(ctx.path_base, "/tmp/aemeath-status-line/subdir");
            assert_eq!(ctx.workspace_root, "/tmp/aemeath-status-line");
            assert_eq!(ctx.raw_path_base, path_base);
            assert_eq!(ctx.raw_workspace_root, workspace_root);
            assert_eq!(
                ctx.workspace.path_base,
                PathBuf::from("/tmp/aemeath-status-line/subdir")
            );
            assert_eq!(
                ctx.workspace.workspace_root,
                PathBuf::from("/tmp/aemeath-status-line")
            );
            assert!(ctx.workspace.context_stack.is_empty());
        }
        _ => panic!("expected WorkingDirectoryChanged event"),
    }
}
