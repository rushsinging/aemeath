use super::*;

fn make_tool(name: &str, description: &str) -> ToolInfo {
    ToolInfo {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: serde_json::json!({"type": "object"}),
        is_read_only: true,
    }
}

#[test]
fn test_compute_relevance_exact_name_match() {
    let tool = make_tool("Bash", "Execute shell commands");
    assert_eq!(compute_relevance("bash", &tool), Some(100.0));
}

#[test]
fn test_compute_relevance_name_contains() {
    let tool = make_tool("ToolSearch", "Search for tools");
    assert_eq!(compute_relevance("search", &tool), Some(80.0));
}

#[test]
fn test_compute_relevance_desc_contains() {
    let tool = make_tool("Bash", "Execute shell commands");
    assert_eq!(compute_relevance("shell", &tool), Some(50.0));
}

#[test]
fn test_compute_relevance_no_match() {
    let tool = make_tool("Bash", "Execute shell commands");
    assert_eq!(compute_relevance("file", &tool), None);
}

#[test]
fn test_compute_relevance_case_insensitive() {
    let tool = make_tool("Read", "Read file contents");
    // query 应该是小写的（调用方已转换）
    assert_eq!(compute_relevance("read", &tool), Some(100.0));
    assert_eq!(compute_relevance("read file", &tool), Some(50.0));
}
