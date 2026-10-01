use super::*;

#[test]
fn test_storage_adapter_new_wraps_inner() {
    let adapter = StorageAdapter::new("storage");

    assert_eq!(adapter.0, "storage");
}
