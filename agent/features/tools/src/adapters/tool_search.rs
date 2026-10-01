use crate::domain::types::tool_search::{ToolInfo, ToolSearchInput, ToolSearchResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;

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
        share::i18n::tools::core::tool_search("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::tool_search(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        ToolSearchInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
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
        let tools: Vec<ToolInfo> = match ctx.catalog_query() {
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
#[path = "tool_search_tests.rs"]
mod tests;
