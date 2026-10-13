use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::{display_path, typed_data};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::{build_header_line, format_goal_header, goal_or_fallback};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sdk::tool_input::{GlobInput, GrepInput};
use std::path::Path;

/// Deserialize a typed Input from a raw `serde_json::Value`, tolerating
/// missing / malformed fields via `Default`.
fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
}

fn content_is_glob_shape(content: Option<&serde_json::Value>) -> bool {
    let Some(value) = content else {
        return false;
    };
    value.get("files").is_some() || value.get("count").is_some()
}

fn content_is_grep_shape(content: Option<&serde_json::Value>) -> bool {
    let Some(value) = content else {
        return false;
    };
    value.get("files").is_some()
        || value.get("matches").is_some()
        || value.get("total_matches").is_some()
        || value.get("total_files").is_some()
        || value.get("query").is_some()
        || value.get("shown").is_some()
}

// ── Glob ─────────────────────────────────────────────────────────

struct GlobDisplay;
impl ToolDisplay for GlobDisplay {
    fn name(&self) -> &str {
        "Glob"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<GlobInput>(input);
        format_goal_header(self.display_name(), &args.goal, &args.pattern)
    }
    fn header_for_subagent(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> String {
        self.format_header(input, workspace_root)
    }
    /// result 到达后，从 `GlobResult.count` 反序列化读取匹配文件数。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let args = parse_input::<GlobInput>(input);
        let n = typed_data::<sdk::tool_result::GlobResult>(result_payload).map(|r| r.count);
        let suffix = n.map(|c| format!(" ({c} files)")).unwrap_or_default();
        let header_text = goal_or_fallback(&args.goal, &args.pattern);
        if header_text.is_empty() && suffix.is_empty() {
            Line::from(Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        } else {
            build_header_line(self.display_name(), &header_text, &suffix)
        }
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let args = parse_input::<GlobInput>(input);
        let mut lines = Vec::new();
        if !args.pattern.is_empty() {
            lines.push(args.pattern);
        }
        if let Some(path) = args.path.as_ref().filter(|path| !path.is_empty()) {
            lines.push(path.clone());
        }
        lines
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        use super::super::common::typed_from_content;
        let result: sdk::tool_result::GlobResult = typed_from_content(content)?;
        if !result.files.is_empty() {
            return Some(result.files);
        }
        // 仅当 payload 明确是 GlobResult 形状时显示空占位；
        // 否则回退 Plain text（兼容仅 text 的旧夹具 / 历史 session）。
        if content_is_glob_shape(content) {
            Some(vec!["（无匹配文件）".to_string()])
        } else {
            None
        }
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
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
    name: "Glob",
    display: || Box::new(GlobDisplay)
});

// ── Grep ─────────────────────────────────────────────────────────

struct GrepDisplay;
impl ToolDisplay for GrepDisplay {
    fn name(&self) -> &str {
        "Grep"
    }
    fn format_header(&self, input: &serde_json::Value, workspace_root: Option<&Path>) -> String {
        let args = parse_input::<GrepInput>(input);
        let path = args
            .path
            .as_deref()
            .map(|path| display_path(path, workspace_root))
            .filter(|path| !path.is_empty())
            .unwrap_or_default();
        let fallback = match (args.pattern.is_empty(), path.is_empty()) {
            (true, true) => String::new(),
            (false, true) => args.pattern.clone(),
            (true, false) => path,
            (false, false) => format!("{} in {}", args.pattern, path),
        };
        format_goal_header(self.display_name(), &args.goal, &fallback)
    }
    fn header_for_subagent(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> String {
        self.format_header(input, workspace_root)
    }
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let args = parse_input::<GrepInput>(input);
        let path = args.path.as_deref().unwrap_or(".");
        let rel = display_path(path, workspace_root);
        let fallback = if args.pattern.is_empty() {
            format!("in {rel}")
        } else {
            format!("/{}/, path={rel}", args.pattern)
        };
        let header_text = goal_or_fallback(&args.goal, &fallback);
        let n = typed_data::<sdk::tool_result::GrepResult>(result_payload).map(|r| r.total_matches);
        let suffix = n.map(|c| format!(" ({c} matches)")).unwrap_or_default();
        build_header_line(self.display_name(), &header_text, &suffix)
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let args = parse_input::<GrepInput>(input);
        let mut lines = Vec::new();
        if !args.pattern.is_empty() {
            lines.push(args.pattern);
        }
        if let Some(path) = args.path.as_ref().filter(|path| !path.is_empty()) {
            lines.push(path.clone());
        }
        lines
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        use super::super::common::typed_from_content;
        let result: sdk::tool_result::GrepResult = typed_from_content(content)?;
        if !result.files.is_empty() {
            return Some(
                result
                    .files
                    .iter()
                    .map(|file| format!("{} · {}", file.file_path, file.match_count))
                    .collect(),
            );
        }
        if !result.matches.is_empty() {
            return Some(
                result
                    .matches
                    .iter()
                    .map(|hit| {
                        format!(
                            "{}:{}: {}",
                            hit.file_path.display(),
                            hit.line_number,
                            hit.line.trim()
                        )
                    })
                    .collect(),
            );
        }
        // 空结构化结果：仅 typed 形状显示占位，否则回退 Plain text。
        if content_is_grep_shape(content) {
            Some(vec!["（无命中）".to_string()])
        } else {
            None
        }
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
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
    name: "Grep",
    display: || Box::new(GrepDisplay)
});
