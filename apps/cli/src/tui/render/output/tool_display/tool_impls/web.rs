//! WebFetch / WebSearch TUI 显示：友好 header + typed result 摘要行。

use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::{truncate_ellipsis, typed_data, typed_from_content};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::build_header_line;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sdk::tool_input::{WebFetchInput, WebSearchInput};
use sdk::tool_result::{WebFetchResult, WebSearchResult};
use std::path::Path;

fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
}

fn family_policy(max_lines: usize) -> ToolRenderPolicy {
    ToolRenderPolicy {
        header: HeaderPolicy::Standard,
        details: DetailsPolicy::Hidden,
        result: ResultPolicy::Visible {
            max_lines: Some(max_lines),
            render_kind: ResultRender::Plain,
            tail_mode: false,
        },
    }
}

// ── WebFetch ─────────────────────────────────────────────────────

struct WebFetchDisplay;
impl ToolDisplay for WebFetchDisplay {
    fn name(&self) -> &str {
        "WebFetch"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<WebFetchInput>(input);
        if args.url.is_empty() {
            self.display_name().to_string()
        } else {
            let display_url = truncate_ellipsis(&args.url, 60);
            format!("{} {display_url}", self.display_name())
        }
    }
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let args = parse_input::<WebFetchInput>(input);
        let result: Option<WebFetchResult> = typed_data(result_payload);
        let suffix = result
            .filter(|r| r.truncated)
            .map(|_| " (truncated)".to_string())
            .unwrap_or_default();
        if args.url.is_empty() && suffix.is_empty() {
            Line::from(Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        } else {
            build_header_line(self.display_name(), &args.url, &suffix)
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: WebFetchResult = typed_from_content(content)?;
        let mut lines = Vec::new();
        let title = result.title.trim();
        if !title.is_empty() {
            lines.push(truncate_ellipsis(title, 80));
        }
        let preview = result
            .content
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("");
        if !preview.is_empty() {
            lines.push(truncate_ellipsis(preview, 120));
        }
        let mut meta = Vec::new();
        if result.truncated {
            meta.push("truncated".to_string());
        }
        if !result.links.is_empty() {
            meta.push(format!("links: {}", result.links.len()));
        }
        if !meta.is_empty() {
            lines.push(meta.join(" · "));
        }
        if lines.is_empty() {
            lines.push("（无内容）".to_string());
        }
        Some(lines)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(8)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "WebFetch",
    display: || Box::new(WebFetchDisplay)
});

// ── WebSearch ────────────────────────────────────────────────────

struct WebSearchDisplay;
impl ToolDisplay for WebSearchDisplay {
    fn name(&self) -> &str {
        "WebSearch"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<WebSearchInput>(input);
        let query = args.query.trim();
        if query.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {}", self.display_name(), truncate_ellipsis(query, 60))
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: WebSearchResult = typed_from_content(content)?;
        if result.results.is_empty() {
            return Some(vec!["（无结果）".to_string()]);
        }
        Some(
            result
                .results
                .iter()
                .map(|hit| {
                    let title = hit
                        .get("title")
                        .and_then(|value| value.as_str())
                        .unwrap_or("-");
                    let url = hit
                        .get("url")
                        .and_then(|value| value.as_str())
                        .unwrap_or("-");
                    let snippet = hit
                        .get("snippet")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .trim();
                    if snippet.is_empty() {
                        format!("{} · {}", truncate_ellipsis(title, 60), url)
                    } else {
                        format!(
                            "{} · {} · {}",
                            truncate_ellipsis(title, 40),
                            truncate_ellipsis(url, 40),
                            truncate_ellipsis(snippet, 60)
                        )
                    }
                })
                .collect(),
        )
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(10)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "WebSearch",
    display: || Box::new(WebSearchDisplay)
});

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
