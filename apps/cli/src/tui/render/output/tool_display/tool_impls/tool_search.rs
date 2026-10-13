//! ToolSearch TUI 显示：query header + 工具摘要行（不含 input_schema）。

use super::super::common::{truncate_ellipsis, typed_from_content};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::format_goal_header;
use sdk::tool_input::ToolSearchInput;
use sdk::tool_result::ToolSearchResult;
use std::path::Path;

fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
}

struct ToolSearchDisplay;
impl ToolDisplay for ToolSearchDisplay {
    fn name(&self) -> &str {
        "ToolSearch"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<ToolSearchInput>(input);
        let fallback = truncate_ellipsis(args.query.trim(), 60);
        format_goal_header(self.display_name(), &args.goal, &fallback)
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let args = parse_input::<ToolSearchInput>(input);
        let query = args.query.trim();
        if query.is_empty() {
            vec![]
        } else {
            vec![query.to_string()]
        }
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: ToolSearchResult = typed_from_content(content)?;
        if result.tools.is_empty() {
            return Some(vec!["（无匹配）".to_string()]);
        }
        Some(
            result
                .tools
                .iter()
                .map(|tool| {
                    let kind = match tool.kind {
                        sdk::tool_result::SearchItemKind::Tool => "tool",
                        sdk::tool_result::SearchItemKind::Skill => "skill",
                    };
                    let access = if tool.is_read_only { "ro" } else { "rw" };
                    format!(
                        "{} · {} · {} · {}",
                        kind,
                        tool.name,
                        access,
                        truncate_ellipsis(tool.description.trim(), 80)
                    )
                })
                .collect(),
        )
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Compact,
            details: DetailsPolicy::Expanded,
            result: ResultPolicy::Visible {
                max_lines: Some(15),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "ToolSearch",
    display: || Box::new(ToolSearchDisplay)
});

#[cfg(test)]
#[path = "tool_search_tests.rs"]
mod tests;
