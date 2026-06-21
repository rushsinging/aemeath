use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::tool_search::{ToolInfo, ToolSearchInput, ToolSearchResult};

/// ToolSearch tool - dynamically searches available tools from the registry.
///
/// Returns detailed tool info (name, description, input_schema, is_read_only)
/// sorted by relevance: exact name match > name contains > description contains.
pub struct ToolSearchTool;

#[async_trait]
impl TypedTool for ToolSearchTool {
    type Output = ToolSearchResult;
    fn name(&self) -> &str {
        "ToolSearch"
    }
    fn description(&self) -> &str {
        "Search for available tools by name or functionality. Use this to discover tools that can help with specific tasks."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::tool_search(lang))
    }
    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        ToolSearchInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        ToolSearchResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<ToolSearchResult> {
        let args: ToolSearchInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let query = args.query.to_lowercase();

        // 从注册表动态获取工具列表
        let tools: Vec<ToolInfo> = match &ctx.registry {
            Some(reg) => reg
                .tool_names()
                .into_iter()
                .filter_map(|name| reg.tool_info(&name))
                .collect(),
            None => Vec::new(),
        };

        if query.is_empty() {
            let count = tools.len();
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            return TypedToolResult::success(
                format!("Available tools ({count})\n{}", names.join("\n")),
                ToolSearchResult { tools },
            );
        }

        // 搜索并按相关度排序
        let mut matching: Vec<(ToolInfo, f64)> = tools
            .into_iter()
            .filter_map(|tool| {
                let score = compute_relevance(&query, &tool)?;
                Some((tool, score))
            })
            .collect();

        // 按分数降序排序
        matching.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if matching.is_empty() {
            return TypedToolResult::success(
                format!("No tools found matching '{query}'"),
                ToolSearchResult { tools: vec![] },
            );
        }

        let count = matching.len();
        let names: Vec<String> = matching.iter().map(|(t, _)| t.name.clone()).collect();
        let result_tools: Vec<ToolInfo> = matching.into_iter().map(|(t, _)| t).collect();
        TypedToolResult::success(
            format!(
                "Found {count} tool(s) matching '{query}'\n{}",
                names.join("\n")
            ),
            ToolSearchResult {
                tools: result_tools,
            },
        )
    }
}

/// 计算工具与查询的相关度分数。返回 None 表示不匹配。
///
/// 分数规则：
/// - 名称完全匹配：100
/// - 名称包含查询：80
/// - 描述包含查询：50
fn compute_relevance(query: &str, tool: &ToolInfo) -> Option<f64> {
    let name_lower = tool.name.to_lowercase();
    let desc_lower = tool.description.to_lowercase();

    if name_lower == query {
        Some(100.0)
    } else if name_lower.contains(query) {
        Some(80.0)
    } else if desc_lower.contains(query) {
        Some(50.0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
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
}
