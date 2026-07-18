use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::{display_path, typed_data};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::{build_header_line, truncate_path};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sdk::tool_input::{GlobInput, GrepInput};
use std::path::Path;

/// Deserialize a typed Input from a raw `serde_json::Value`, tolerating
/// missing / malformed fields via `Default`.
fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
}

// ── Glob ─────────────────────────────────────────────────────────

struct GlobDisplay;
impl ToolDisplay for GlobDisplay {
    fn name(&self) -> &str {
        "Glob"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<GlobInput>(input);
        if args.pattern.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {}", self.display_name(), args.pattern)
        }
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
        if args.pattern.is_empty() && suffix.is_empty() {
            Line::from(Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        } else {
            build_header_line(self.display_name(), &args.pattern, &suffix)
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
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
        let path = args.path.as_deref().unwrap_or(".");
        let rel = display_path(path, workspace_root);
        let display_path = truncate_path(&rel, 40);
        if args.pattern.is_empty() {
            format!("{} in {display_path}", self.display_name())
        } else {
            format!(
                "{} /{}/ in {display_path}",
                self.display_name(),
                args.pattern
            )
        }
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
        let arg = if args.pattern.is_empty() {
            format!("in {rel}")
        } else {
            format!("/{}/, path={rel}", args.pattern)
        };
        let n = typed_data::<sdk::tool_result::GrepResult>(result_payload).map(|r| r.total_matches);
        let suffix = n.map(|c| format!(" ({c} matches)")).unwrap_or_default();
        build_header_line(self.display_name(), &arg, &suffix)
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
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
