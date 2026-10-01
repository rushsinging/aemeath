use super::*;

#[test]
fn test_fs_adapter_new_wraps_inner() {
    let adapter = FsAdapter::new("fs");

    assert_eq!(adapter.0, "fs");
}
