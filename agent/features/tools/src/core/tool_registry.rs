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

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    /// 注册一个工具（自动包裹 [`TypedToolAdapter`]）。
    ///
    /// 所有工具统一实现 [`TypedTool`]；registry 内部自动适配为 `dyn Tool`
    /// 存入 `HashMap`。工具名（key）由 `TypedTool::name()` 决定。
    pub fn register<T: TypedTool + 'static>(&self, tool: T) {
        let adapter = TypedToolAdapter::new(tool);
        let name = adapter.name().to_string();
        self.tools.write().insert(name, Arc::new(adapter));
    }

    pub fn unregister(&self, name: &str) -> bool {
        self.tools.write().remove(name).is_some()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.read().contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.tools.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.read().is_empty()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().get(name).cloned()
    }

    pub fn schemas(&self) -> Vec<Value> {
        self.tools
            .read()
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
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
}
