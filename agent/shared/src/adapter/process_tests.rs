use super::*;

#[test]
fn test_process_adapter_new_wraps_inner() {
    let adapter = ProcessAdapter::new("process");

    assert_eq!(adapter.0, "process");
}
