use super::{display_status_path, display_working_dir};
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
