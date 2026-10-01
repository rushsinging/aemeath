use super::*;
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;

struct DummyTool {
    name: String,
    description: String,
    concurrency_safe: bool,
}

impl DummyTool {
    fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            concurrency_safe: true,
        }
    }

    fn sequential(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            concurrency_safe: false,
        }
    }
}

#[async_trait]
impl TypedTool for DummyTool {
    type Output = Value;

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }

    fn is_concurrency_safe(&self) -> bool {
        self.concurrency_safe
    }

    async fn call(
        &self,
        _input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        TypedToolResult::success("ok", Value::Null)
    }
}

#[test]
fn test_tool_registry_unregister_existing_tool() {
    let registry = ToolRegistry::new();
    registry.register(DummyTool::new("dummy", "first"));

    assert!(registry.contains("dummy"));
    assert_eq!(registry.len(), 1);
    assert!(registry.unregister("dummy"));
    assert!(!registry.contains("dummy"));
    assert!(registry.is_empty());
}

#[test]
fn test_tool_registry_unregister_missing_tool() {
    let registry = ToolRegistry::new();

    assert!(!registry.unregister("missing"));
    assert!(registry.is_empty());
}

#[test]
fn test_tool_registry_register_overwrites_existing_tool() {
    let registry = ToolRegistry::new();
    registry.register(DummyTool::new("dummy", "first"));
    registry.register(DummyTool::new("dummy", "second"));

    assert_eq!(registry.len(), 1);
    assert_eq!(registry.get("dummy").unwrap().description(), "second");
}

#[test]
fn test_tool_registry_lookup_is_case_insensitive() {
    let registry = ToolRegistry::new();
    registry.register(DummyTool::new("Read", "file read tool"));

    assert!(registry.get("read").is_some());
    assert!(registry.get("READ").is_some());
    assert!(registry.get("Read").is_some());
    assert!(registry.contains("read"));
    assert!(registry.contains("READ"));
    assert!(registry.get("write").is_none());
}

#[test]
fn test_tool_registry_duplicate_different_case_is_same_key() {
    let registry = ToolRegistry::new();
    registry.register(DummyTool::new("Bash", "first"));
    registry.register(DummyTool::new("BASH", "second"));

    assert_eq!(registry.len(), 1);
    assert_eq!(registry.get("bash").unwrap().description(), "second");
}

#[test]
fn test_tool_registry_unregister_is_case_insensitive() {
    let registry = ToolRegistry::new();
    registry.register(DummyTool::new("Edit", "file edit tool"));

    assert!(registry.unregister("EDIT"));
    assert!(!registry.contains("edit"));
    assert!(registry.is_empty());
}

#[test]
fn test_tool_registry_preserves_mcp_qualified_name_with_underscores() {
    // MCP 工具 key 形如 mcp__server__Tool —— 小写化不应破坏跨段语义（查找仍命中）
    let registry = ToolRegistry::new();
    registry.register(DummyTool::new("mcp__Server__Tool", "mcp tool"));

    assert!(registry.get("mcp__server__tool").is_some());
    assert!(registry.get("MCP__SERVER__TOOL").is_some());
    assert_eq!(registry.len(), 1);
}

#[test]
fn schemas_for_exposes_parallel_safe_hint_to_llm() {
    let registry = ToolRegistry::new();
    registry.register(DummyTool::new("parallel", "Parallel tool"));

    let schemas = registry.schemas_for("en");

    let description = schemas[0]["description"].as_str().unwrap();
    assert!(description.contains("Concurrency: Parallel-safe"));
    assert!(description.contains("SAME response"));
    assert!(description.contains("run concurrently"));
}

#[test]
fn schemas_for_exposes_sequential_only_hint_to_llm() {
    let registry = ToolRegistry::new();
    registry.register(DummyTool::sequential("sequential", "Sequential tool"));

    let schemas = registry.schemas_for("en");

    let description = schemas[0]["description"].as_str().unwrap();
    assert!(description.contains("Concurrency: Sequential-only"));
    assert!(description.contains("Do not run multiple calls"));
    assert!(description.contains("preserve order"));
}
