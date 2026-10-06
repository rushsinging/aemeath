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
            .iter()
            .filter_map(|tool| {
                let score = compute_relevance(&query, tool)?;
                Some((tool.clone(), score))
            })
            .collect();

        // 按分数降序排序
        matching.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // #1835：词法高置信短路（exact/name contains 命中）直接用词法序；
        // 低置信（仅 desc contains）或零命中时交 System One 语义重排，失败静默回退。
        let lexical_scores: Vec<f64> = matching.iter().map(|(_, score)| *score).collect();
        let scoring_port = ctx.scoring();
        let result_tools: Vec<ToolInfo> = if scoring_port.is_some()
            && !crate::domain::tool_search_scoring::is_lexical_confident(&lexical_scores)
        {
            let candidates: Vec<ToolInfo> = if matching.is_empty() {
                tools.clone()
            } else {
                matching.iter().map(|(tool, _)| tool.clone()).collect()
            };
            log::debug!(
                target: crate::LOG_TARGET,
                "tool_search_scoring_triggered query={query} candidates={} zero_hit={}",
                candidates.len(),
                matching.is_empty(),
            );
            match semantic_rerank(&query, &candidates, scoring_port.as_ref().expect("checked"))
                .await
            {
                Some(reranked) => reranked,
                None => matching.into_iter().map(|(tool, _)| tool).collect(),
            }
        } else {
            matching.into_iter().map(|(tool, _)| tool).collect()
        };

        if result_tools.is_empty() {
            return TypedToolResult::success(
                format!("No tools found matching '{query}'"),
                ToolSearchResult { tools: vec![] },
            );
        }

        let count = result_tools.len();
        let names: Vec<String> = result_tools.iter().map(|t| t.name.clone()).collect();
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

/// 低置信路径：候选交 System One Choice 语义重排；失败/不可用返回 None（调用方回退词法序）。
async fn semantic_rerank(
    query: &str,
    candidates: &[ToolInfo],
    port: &std::sync::Arc<dyn systemone::ScoringPort>,
) -> Option<Vec<ToolInfo>> {
    use systemone::{ScoringQuestion, ScoringState};

    if candidates.len() < 2 {
        return None;
    }
    let state = ScoringState::new(query.to_owned())?;
    let criteria: Vec<(String, String)> = candidates
        .iter()
        .enumerate()
        .take(systemone::ScoringQuestion::CHOICE_CRITERIA_MAX)
        .map(|(index, tool)| {
            let text: String = format!("{}: {}", tool.name, tool.description)
                .chars()
                .take(crate::constants::TOOL_SEARCH_CRITERIA_MAX_CHARS)
                .collect();
            (index.to_string(), text)
        })
        .collect();
    let question = ScoringQuestion::choice(crate::constants::TOOL_SEARCH_INSTRUCTIONS, criteria)
        .expect("criteria 数量与内容已受本函数约束，构造不可失败");
    match port.answer(&state, &[question]).await {
        Ok(answers) => match answers.first() {
            Some(systemone::ScoringAnswer::Choice { probabilities, .. }) => {
                Some(crate::domain::tool_search_scoring::apply_scoring_order(
                    candidates.to_vec(),
                    probabilities.as_slice(),
                ))
            }
            _ => None,
        },
        Err(unavailable) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "tool_search_scoring_fallback reason={unavailable}"
            );
            None
        }
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
