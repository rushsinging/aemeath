use crate::domain::tool::TypedToolAdapter;
use crate::domain::{Tool, ToolCapabilities, ToolName, TypedTool};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    capabilities: RwLock<HashMap<ToolName, ToolCapabilities>>,
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

fn description_with_concurrency_hint(description: &str, concurrency_safe: bool) -> String {
    let hint = if concurrency_safe {
        "Concurrency: Parallel-safe. If multiple calls to this tool are independent, issue them in the SAME response so they can run concurrently."
    } else {
        "Concurrency: Sequential-only. Do not run multiple calls to this tool in parallel; preserve order."
    };

    format!("{description}\n\n{hint}")
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            capabilities: RwLock::new(HashMap::new()),
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

    pub fn register_with_capabilities<T: TypedTool + 'static>(
        &self,
        tool: T,
        capabilities: ToolCapabilities,
    ) {
        let name = ToolName::new(tool.name());
        self.capabilities.write().insert(name, capabilities);
        self.register(tool);
    }

    pub fn declare_capabilities_for_test(&self, name: &ToolName, capabilities: ToolCapabilities) {
        self.capabilities.write().insert(name.clone(), capabilities);
    }

    pub fn required_capabilities(&self, name: &str) -> Option<ToolCapabilities> {
        self.capabilities.read().get(&ToolName::new(name)).copied()
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
                    "description": description_with_concurrency_hint(
                        &tool.description_for(lang),
                        tool.is_concurrency_safe()
                    ),
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

impl crate::domain::ToolListProvider for ToolRegistry {
    fn tool_names(&self) -> Vec<String> {
        self.names()
    }
    fn tool_description(&self, name: &str) -> Option<String> {
        self.get(name).map(|t| t.description().to_string())
    }
    fn tool_info(&self, name: &str) -> Option<crate::domain::types::tool_search::ToolInfo> {
        self.get(name)
            .map(|t| crate::domain::types::tool_search::ToolInfo {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
                is_read_only: t.is_read_only(),
            })
    }
}

#[cfg(test)]
#[path = "tool_registry_tests.rs"]
mod tests;
