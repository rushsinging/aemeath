use super::super::common::{truncate_ellipsis, typed_data};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sdk::tool_input::BashInput;
use std::path::Path;

/// Deserialize a typed Input from a raw `serde_json::Value`, tolerating
/// missing / malformed fields via `Default`.
fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
}

// ── Bash ─────────────────────────────────────────────────────────

struct BashDisplay;
impl ToolDisplay for BashDisplay {
    fn name(&self) -> &str {
        "Bash"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<BashInput>(input);
        if !args.goal.is_empty() {
            format!("{} {}", self.display_name(), args.goal)
        } else if !args.command.is_empty() {
            // fallback：老 session 缺 goal 时显示截断的 command
            format!(
                "{} {}",
                self.display_name(),
                truncate_ellipsis(&args.command, 80)
            )
        } else {
            self.display_name().to_string()
        }
    }
    fn header_for_subagent(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> String {
        self.format_header(input, workspace_root)
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let args = parse_input::<BashInput>(input);
        if args.command.is_empty() {
            Vec::new()
        } else {
            // 命令全文不截断，由渲染层 Word wrap 自动换行
            vec![args.command]
        }
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Expanded,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: true, // 只显示最后 5 行
            },
        }
    }
    /// 当 result 到达后，从 `BashResult.exit_code` / `signal` 读取
    /// exit code 显示后缀：0/None 空；signal 有值 `(signal N)`；> 0 `(exit N)`。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let args = parse_input::<BashInput>(input);
        let result: Option<sdk::tool_result::BashResult> = typed_data(result_payload);
        let suffix = match result {
            Some(r) if r.exit_code != 0 => {
                if let Some(sig) = r.signal {
                    format!(" (signal {sig})")
                } else {
                    format!(" (exit {})", r.exit_code)
                }
            }
            _ => String::new(),
        };
        let header_text = if !args.goal.is_empty() {
            args.goal
        } else if !args.command.is_empty() {
            // fallback：老 session 缺 goal 时显示截断的 command
            truncate_ellipsis(&args.command, 80)
        } else {
            String::new()
        };
        if header_text.is_empty() && suffix.is_empty() {
            Line::from(Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        } else if header_text.is_empty() {
            Line::from(vec![
                Span::styled(
                    self.display_name().to_string(),
                    Style::default().fg(theme::ACCENT_BRIGHT),
                ),
                Span::styled(suffix, Style::default().fg(theme::TEXT_MUTED)),
            ])
        } else {
            let mut spans = vec![
                Span::styled(
                    self.display_name().to_string(),
                    Style::default().fg(theme::ACCENT_BRIGHT),
                ),
                Span::raw(format!(" {header_text}")),
            ];
            if !suffix.is_empty() {
                spans.push(Span::styled(suffix, Style::default().fg(theme::TEXT_MUTED)));
            }
            Line::from(spans)
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Bash",
    display: || Box::new(BashDisplay)
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_shows_goal_when_present() {
        let display = BashDisplay;
        let input = serde_json::json!({
            "goal": "运行测试",
            "command": "cargo test -- --nocapture"
        });
        let header = display.format_header(&input, None);
        assert!(
            header.contains("运行测试"),
            "header 应包含 goal，实际: {header}"
        );
        // header 不含命令全文
        assert!(
            !header.contains("cargo test"),
            "header 不应包含命令全文，实际: {header}"
        );
    }

    #[test]
    fn header_falls_back_to_command_when_goal_empty() {
        let display = BashDisplay;
        let input = serde_json::json!({
            "goal": "",
            "command": "cargo build"
        });
        let header = display.format_header(&input, None);
        // goal 为空时 fallback：header 显示截断的 command
        assert!(
            header.contains("cargo build"),
            "goal 为空时 header 应 fallback 显示 command，实际: {header}"
        );
    }

    #[test]
    fn details_show_full_command_untruncated() {
        let display = BashDisplay;
        let long_command = "echo hello world && ".repeat(20);
        let input = serde_json::json!({
            "goal": "测试长命令",
            "command": long_command
        });
        let details = display.format_details(&input);
        assert_eq!(details.len(), 1, "details 应只有一行命令全文");
        // 命令全文在 details 中，不截断
        assert_eq!(details[0], long_command);
    }

    #[test]
    fn format_header_line_with_result_uses_goal() {
        let display = BashDisplay;
        let input = serde_json::json!({
            "goal": "构建项目",
            "command": "cargo build --release"
        });
        let line = display.format_header_line_with_result(&input, None, None);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("构建项目"),
            "header line 应包含 goal，实际: {text}"
        );
        assert!(
            !text.contains("cargo build --release"),
            "header line 不应包含命令全文，实际: {text}"
        );
    }

    #[test]
    fn format_header_line_with_result_shows_exit_suffix() {
        let display = BashDisplay;
        let input = serde_json::json!({
            "goal": "失败的命令",
            "command": "false"
        });
        // 模拟 exit_code=1 的 BashResult
        let result_content = serde_json::json!({
            "stdout": "",
            "stderr": "",
            "exit_code": 1,
            "signal": null,
            "path_base": null,
        });
        let payload = ToolResultPayload::new(String::new(), result_content, true, 0);
        let line = display.format_header_line_with_result(&input, Some(&payload), None);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("(exit 1)"),
            "header line 应包含 exit suffix，实际: {text}"
        );
        assert!(
            text.contains("失败的命令"),
            "header line 应包含 goal，实际: {text}"
        );
    }

    #[test]
    fn format_header_line_with_result_falls_back_when_goal_empty() {
        let display = BashDisplay;
        let input = serde_json::json!({
            "goal": "",
            "command": "ls -la"
        });
        let line = display.format_header_line_with_result(&input, None, None);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // goal 为空时 fallback 到 command
        assert!(
            text.contains("ls -la"),
            "goal 为空时 header line 应 fallback 到 command，实际: {text}"
        );
    }

    #[test]
    fn header_for_subagent_uses_goal() {
        let display = BashDisplay;
        let input = serde_json::json!({
            "goal": "子代理任务",
            "command": "echo test"
        });
        let header = display.header_for_subagent(&input, None);
        assert!(
            header.contains("子代理任务"),
            "subagent header 应包含 goal，实际: {header}"
        );
    }
}
