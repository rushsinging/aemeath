use super::*;

#[test]
fn test_mcp_adapter_new_wraps_inner() {
    let adapter = McpAdapter::new("mcp");

    assert_eq!(adapter.0, "mcp");
}
