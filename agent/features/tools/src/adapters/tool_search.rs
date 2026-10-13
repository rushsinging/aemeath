use std::sync::Arc;

use crate::domain::types::tool_search::{
    SearchItemKind, ToolInfo, ToolSearchInput, ToolSearchResult,
};
use crate::domain::{
    SkillCatalogPort, SkillQuery, ToolExecutionContext, TypedTool, TypedToolResult,
};
use async_trait::async_trait;
use serde_json::Value;

/// ToolSearch tool - dynamically searches available tools and skills.
///
/// Returns detailed info (name, description, input_schema, is_read_only, kind)
/// sorted by relevance: exact name match > name contains > description contains.
pub struct ToolSearchTool {
    skills: Arc<dyn SkillCatalogPort>,
}

impl ToolSearchTool {
    pub fn new(skills: Arc<dyn SkillCatalogPort>) -> Self {
        Self { skills }
    }

    fn list_skills(&self, ctx: &ToolExecutionContext) -> Vec<ToolInfo> {
        let snapshot = ctx.skill_query();
        let query = SkillQuery::new(
            ctx.workspace_read().current_workspace_root(),
            snapshot.extra_dirs.clone(),
            snapshot.available_tools.clone(),
        );
        self.skills
            .list(query)
            .into_iter()
            .map(|descriptor| ToolInfo {
                name: descriptor.name().to_string(),
                description: descriptor.description().to_string(),
                input_schema: serde_json::json!({ "type": "object", "properties": {} }),
                is_read_only: true,
                kind: SearchItemKind::Skill,
            })
            .collect()
    }
}

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
            // 空 query 只列 tools，避免 skills 全量倾倒。
            let count = tools.len();
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            return TypedToolResult::success(
                format!("Available tools ({count})\n{}", names.join("\n")),
                ToolSearchResult { tools },
            );
        }

        let mut candidates = tools;
        candidates.extend(self.list_skills(ctx));

        // 搜索并按相关度排序
        let mut matching: Vec<(ToolInfo, f64)> = candidates
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
        let result_tools: Vec<ToolInfo> = match (
            scoring_port.as_ref(),
            crate::domain::tool_search_scoring::is_lexical_confident(&lexical_scores),
        ) {
            (Some(port), false) => {
                let rerank_candidates: Vec<ToolInfo> = if matching.is_empty() {
                    candidates.clone()
                } else {
                    matching.iter().map(|(tool, _)| tool.clone()).collect()
                };
                log::debug!(
                    target: crate::LOG_TARGET,
                    "tool_search_scoring_triggered query={query} candidates={} zero_hit={}",
                    rerank_candidates.len(),
                    matching.is_empty(),
                );
                match semantic_rerank(&query, &rerank_candidates, port).await {
                    Some(reranked) => reranked,
                    None => matching.into_iter().map(|(tool, _)| tool).collect(),
                }
            }
            _ => matching.into_iter().map(|(tool, _)| tool).collect(),
        };

        if result_tools.is_empty() {
            return TypedToolResult::success(
                format!("No tools or skills found matching '{query}'"),
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
            let kind = match tool.kind {
                SearchItemKind::Tool => "tool",
                SearchItemKind::Skill => "skill",
            };
            let text: String = format!("{kind} {}: {}", tool.name, tool.description)
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
