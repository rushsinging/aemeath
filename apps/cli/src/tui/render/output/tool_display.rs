mod common;
mod task_impls;
mod tool_impls;

use crate::tui::render::theme;
use crate::tui::view_model::tool_name::tool_display_name;
use common::truncate_json;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::collections::HashMap;
use std::sync::LazyLock;

// ── ToolRenderPolicy 系统 ──────────────────────────────────────────

/// Header 渲染策略
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderPolicy {
    /// 标准 header：带 ● marker
    Standard,
    /// 紧凑 header：单行，无 marker（如 TaskUpdate）
    Compact,
    /// 自定义图标：用指定 emoji（如 📋 EnterPlanMode）
    CustomIcon(&'static str),
}

/// Details 渲染策略
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailsPolicy {
    /// 展开显示 details
    Expanded,
    /// 隐藏 details
    Hidden,
}

/// Result 渲染策略
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultPolicy {
    /// 不显示 result 子块（如 Read/Write/Edit）
    Hidden,
    /// 显示 result 子块
    Visible {
        /// 最大行数（None 表示全部显示，如 Edit diff）
        max_lines: Option<usize>,
        /// 渲染类型
        render_kind: ResultRender,
        /// tail 模式：只显示最后 N 行（如 Bash）
        tail_mode: bool,
    },
}

/// 工具的渲染策略配置
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolRenderPolicy {
    pub header: HeaderPolicy,
    pub details: DetailsPolicy,
    pub result: ResultPolicy,
}

/// 工具 result 的渲染类型。由工具**显式声明**（`ToolDisplay::render_policy`），渲染层据此
/// 分发，不按 `---DIFF---` 字符或硬编码工具名猜测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultRender {
    /// 纯文本原样预览：保留文件/命令输出原文（含行号/缩进），不做 markdown 重渲染。
    /// 适用于 Read/Bash/Grep 等——避免文件内容里的 markdown（表格/标题）被渲染变形。
    Plain,
    /// unified diff：解析 Edit 结果的 `---DIFF---` 渲染为加减色 diff。
    Diff,
}

// ── ToolDisplay trait ──────────────────────────────────────────────

/// Trait for customizing how a tool call is displayed in the TUI output area.
pub trait ToolDisplay: Send + Sync {
    /// Tool name as registered in the tool registry.
    fn name(&self) -> &str;

    /// 用户可见的 display name（默认从 `tool_display_name` 映射查表）。
    fn display_name(&self) -> &str {
        tool_display_name(self.name())
    }

    /// Format the header line as plain string.
    /// `input` 是解析后的 JSON。
    fn format_header(&self, input: &serde_json::Value) -> String;

    /// Format the header line as styled `Line`。默认实现将 `format_header` 的输出按
    /// `display_name` 前缀拆分：tool name 用 `ACCENT_BRIGHT`（Mauve）着色突出，
    /// 其余文本保持 raw（由调用方的 line base style 统一赋色）。
    /// 需要对 header 不同部分施加不同颜色的工具可覆写此方法。
    fn format_header_line(&self, input: &serde_json::Value) -> Line<'static> {
        let text = self.format_header(input);
        let name = self.display_name();
        if let Some(rest) = text.strip_prefix(name) {
            Line::from(vec![
                Span::styled(name.to_string(), Style::default().fg(theme::ACCENT_BRIGHT)),
                Span::raw(rest.to_string()),
            ])
        } else {
            // format_header 不以 display_name 开头（如 EnterPlanMode 用 📋 前缀），
            // 保持整体 raw。
            Line::from(Span::raw(text))
        }
    }

    /// Format the header line with optional result summary。
    /// 默认实现忽略 result_summary，直接调用 `format_header_line`。
    /// 需要根据 result 更新 header 的工具（如 Read）可覆写此方法。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        _result_summary: Option<&str>,
    ) -> Line<'static> {
        self.format_header_line(input)
    }

    /// Format detail lines shown below the header.
    fn format_details(&self, input: &serde_json::Value) -> Vec<String>;

    /// 返回该工具的渲染策略。
    fn render_policy(&self) -> ToolRenderPolicy;
}

// ── Registration via inventory ─────────────────────────────────────

pub struct ToolDisplayEntry {
    pub name: &'static str,
    pub display: fn() -> Box<dyn ToolDisplay>,
}

inventory::collect!(ToolDisplayEntry);

static TOOL_DISPLAYS: LazyLock<HashMap<&'static str, Box<dyn ToolDisplay>>> = LazyLock::new(|| {
    let mut map: HashMap<&'static str, Box<dyn ToolDisplay>> = HashMap::new();
    for entry in inventory::iter::<ToolDisplayEntry> {
        map.insert(entry.name, (entry.display)());
    }
    map
});

pub(crate) fn lookup_display(name: &str) -> Option<&'static dyn ToolDisplay> {
    TOOL_DISPLAYS.get(name).map(|display| display.as_ref())
}

/// 返回某工具的渲染策略。未注册的工具回退到默认策略。
pub fn result_policy(name: &str) -> ResultPolicy {
    lookup_display(name)
        .map(|display| display.render_policy().result)
        .unwrap_or(ResultPolicy::Visible {
            max_lines: Some(5),
            render_kind: ResultRender::Plain,
            tail_mode: false,
        })
}

/// 该工具 result 的渲染类型（取自 `ToolDisplay::render_policy`，未注册回退 `Plain`）。
/// 渲染层据此分发，不按 `---DIFF---` 字符或硬编码工具名猜测渲染类型。
pub fn result_render_kind(name: &str) -> ResultRender {
    match result_policy(name) {
        ResultPolicy::Visible { render_kind, .. } => render_kind,
        _ => ResultRender::Plain,
    }
}

/// Format a tool call for human-friendly display.
/// 返回 `(Line, details)`：Line 已含样式，details 为纯文本行。
///
/// `result_summary`：可选的结果摘要，用于在 result 到达后更新 header（如 Read 实际行数）。
pub fn format_tool_call(
    name: &str,
    raw_json: &str,
    result_summary: Option<&str>,
) -> (Line<'static>, Vec<String>) {
    let parsed: serde_json::Value =
        serde_json::from_str(raw_json).unwrap_or(serde_json::Value::Null);

    if let Some(display) = lookup_display(name) {
        let header = display.format_header_line_with_result(&parsed, result_summary);
        let details = match display.render_policy().details {
            DetailsPolicy::Expanded => display.format_details(&parsed),
            DetailsPolicy::Hidden => vec![],
        };
        return (header, details);
    }

    // Fallback for unknown tools
    let truncated = truncate_json(raw_json);
    (
        Line::from(vec![
            Span::raw("● "),
            Span::styled(
                tool_display_name(name).to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ),
        ]),
        vec![truncated],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 辅助函数：从 Line 中提取纯文本。
    fn line_to_string(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn test_lookup_display_finds_task_list_create() {
        let display = lookup_display("TaskListCreate");
        assert!(
            display.is_some(),
            "TaskListCreate 应在 display registry 中注册"
        );
        assert_eq!(display.unwrap().name(), "TaskListCreate");
    }

    #[test]
    fn test_lookup_display_finds_task_create() {
        assert!(lookup_display("TaskCreate").is_some());
    }

    #[test]
    fn test_lookup_display_finds_task_update() {
        assert!(lookup_display("TaskUpdate").is_some());
    }

    #[test]
    fn test_format_tool_call_task_list_create() {
        let (header, details) = format_tool_call(
            "TaskListCreate",
            r#"{"subject":"修复 bug 84","summary":"修复渲染"}"#,
            None,
        );
        let text = line_to_string(&header);
        assert!(
            text.contains("修复 bug 84"),
            "header 应包含 subject: {text}"
        );
        assert!(
            details.is_empty(),
            "Compact 模式不应显示 details: {details:?}"
        );
    }

    #[test]
    fn test_format_tool_call_task_create_compact_merges_description_into_header() {
        let (header, details) = format_tool_call(
            "TaskCreate",
            r#"{"subject":"分析","description":"查看结构"}"#,
            None,
        );
        let text = line_to_string(&header);
        assert!(text.contains("分析"), "header: {text}");
        assert!(
            text.contains("查看结构"),
            "header 应合并 description: {text}"
        );
        assert!(
            details.is_empty(),
            "Compact 模式不应显示 details: {details:?}"
        );
    }

    #[test]
    fn test_format_tool_call_task_create_compact_no_description() {
        let (header, details) = format_tool_call("TaskCreate", r#"{"subject":"分析"}"#, None);
        let text = line_to_string(&header);
        assert!(text.contains("分析"), "header: {text}");
        assert!(
            !text.contains(':'),
            "无 description 时 header 不应有冒号: {text}"
        );
        assert!(details.is_empty());
    }

    #[test]
    fn test_format_tool_call_task_update_compact_hides_details() {
        let (header, details) =
            format_tool_call("TaskUpdate", r#"{"taskId":"42","status":"completed"}"#, None);
        let text = line_to_string(&header);
        assert!(text.contains("42"), "header 应包含 taskId: {text}");
        assert!(text.contains("completed"), "header 应包含 status: {text}");
        assert!(
            details.is_empty(),
            "Compact 模式不应显示 details: {details:?}"
        );
    }

    #[test]
    fn test_format_tool_call_uses_display_name_in_header() {
        // Bash → Run
        let (header, _) = format_tool_call("Bash", r#"{"command":"ls"}"#, None);
        let text = line_to_string(&header);
        assert!(
            text.starts_with("Run "),
            "Bash header 应使用 display name 'Run': {text}"
        );

        // Grep → Search
        let (header, _) = format_tool_call("Grep", r#"{"pattern":"foo"}"#, None);
        let text = line_to_string(&header);
        assert!(
            text.starts_with("Search "),
            "Grep header 应使用 display name 'Search': {text}"
        );

        // Glob → Find
        let (header, _) = format_tool_call("Glob", r#"{"pattern":"*.rs"}"#, None);
        let text = line_to_string(&header);
        assert!(
            text.starts_with("Find "),
            "Glob header 应使用 display name 'Find': {text}"
        );
    }

    #[test]
    fn test_format_tool_call_unknown_tool_uses_fallback() {
        let (header, details) = format_tool_call("UnknownTool", r#"{"key":"value"}"#, None);
        let text = line_to_string(&header);
        assert_eq!(text, "● UnknownTool");
        assert!(!details.is_empty(), "fallback 应截断 JSON");
    }

    #[test]
    fn test_format_tool_call_invalid_json_uses_fallback() {
        let (header, _details) = format_tool_call("TaskListCreate", "not json", None);
        // 不应 panic，应 fallback。display name 为 "New Task List"。
        let text = line_to_string(&header);
        assert!(text.contains("New Task List"));
    }

    #[test]
    fn test_result_render_kind_diff_only_for_edit() {
        // 渲染类型由工具声明：Edit→Diff，其它工具与未注册工具→Plain（防 ---DIFF--- 误判）。
        assert_eq!(result_render_kind("Edit"), ResultRender::Diff);
        assert_eq!(result_render_kind("Read"), ResultRender::Plain);
        assert_eq!(result_render_kind("UnknownTool"), ResultRender::Plain);
    }

    #[test]
    fn test_result_policy_read_is_hidden() {
        assert_eq!(
            result_policy("Read"),
            ResultPolicy::Hidden,
            "Read 的 result 策略应为 Hidden"
        );
    }

    #[test]
    fn test_result_policy_bash_is_tail_mode() {
        assert_eq!(
            result_policy("Bash"),
            ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: true,
            },
            "Bash 的 result 策略应为 tail 模式"
        );
    }

    #[test]
    fn test_format_tool_call_bash_long_cjk_command_no_panic() {
        // 回归 #218：含中文的超长 Bash 命令按字节切片会落在多字节字符内部触发 panic。
        let cmd = "gh pr create --title 'fix(runtime): 使用人类可读摘要替代 JSON 作为 tool call summary' --body '## 问题'";
        let raw = serde_json::json!({ "command": cmd }).to_string();
        let (header, _details) = format_tool_call("Bash", &raw, None);
        let text = line_to_string(&header);
        assert!(
            text.starts_with("Run "),
            "header 应以 'Run ' 开头 (Bash display name): {text}"
        );
        assert!(
            text.ends_with("..."),
            "超长命令应被截断并以 ... 结尾: {text}"
        );
    }

    #[test]
    fn test_format_tool_call_read_long_cjk_path_no_panic() {
        // 回归 #218：含中文的超长路径经 truncate_path 尾部截断不应 panic。
        let path = format!("/项目/{}/数据/报告为准的文件名.rs", "子目录".repeat(20));
        let raw = serde_json::json!({ "file_path": path }).to_string();
        let (header, _details) = format_tool_call("Read", &raw, None);
        let text = line_to_string(&header);
        assert!(
            text.starts_with("Read "),
            "header 应以 'Read ' 开头: {text}"
        );
        assert!(text.contains("..."), "超长路径应被截断: {text}");
    }
}
