use crate::contract::tool::TypedToolAdapter;
use crate::contract::{Tool, TypedTool};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 工具名规范化：注册与查找使用同一套 key（统一转 ASCII 小写），
/// 保证大小写不同的工具名查找命中、并避免语义重复的注册项。
fn normalize_key(name: &str) -> String {
    name.to_ascii_lowercase()
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    /// 注册一个工具（自动包裹 [`TypedToolAdapter`]）。
    ///
    /// 所有工具统一实现 [`TypedTool`]；registry 内部自动适配为 `dyn Tool`
    /// 存入 `HashMap`。工具名（key）由 `TypedTool::name()` 决定，
    /// 经 [`normalize_key`] 统一小写后作为存储 key。
    pub fn register<T: TypedTool + 'static>(&self, tool: T) {
        let adapter = TypedToolAdapter::new(tool);
        let key = normalize_key(adapter.name());
        self.tools.write().insert(key, Arc::new(adapter));
    }

    pub fn unregister(&self, name: &str) -> bool {
        let key = normalize_key(name);
        self.tools.write().remove(&key).is_some()
    }

    pub fn contains(&self, name: &str) -> bool {
        let key = normalize_key(name);
        self.tools.read().contains_key(&key)
    }

    pub fn len(&self) -> usize {
        self.tools.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.read().is_empty()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let key = normalize_key(name);
        self.tools.read().get(&key).cloned()
    }

    pub fn schemas(&self) -> Vec<Value> {
        self.schemas_for(share::i18n::DEFAULT_LANG)
    }

    /// 按 lang 生成 tool schema（注入 LLM 用）。description 走 `description_for(lang)`，
    /// 未覆盖的工具自动降级到默认语言英文。
    pub fn schemas_for(&self, lang: &str) -> Vec<Value> {
        self.tools
            .read()
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description_for(lang),
                    "input_schema": tool.input_schema(),
                    "data_schema": tool.data_schema(),
                })
            })
            .collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.read().keys().cloned().collect()
    }
}

impl crate::contract::ToolListProvider for ToolRegistry {
    fn tool_names(&self) -> Vec<String> {
        self.names()
    }
    fn tool_description(&self, name: &str) -> Option<String> {
        self.get(name).map(|t| t.description().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{ToolExecutionContext, TypedTool, TypedToolResult};
    use async_trait::async_trait;

    struct DummyTool {
        name: String,
        description: String,
    }

    impl DummyTool {
        fn new(name: &str, description: &str) -> Self {
            Self {
                name: name.to_string(),
                description: description.to_string(),
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
}
