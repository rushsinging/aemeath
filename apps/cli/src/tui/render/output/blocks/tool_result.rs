//! 工具结果子块渲染组件：独占工具结果的富渲染（从 tool_call.rs 迁移而来，#60）。
//!
//! 作为 ToolCall 的 depth-1 子节点，结果行不再自拼缩进/marker——块级缩进由
//! gutter 在组合期注入（续行等宽空白），结构上隔离 #65 的 fence 状态机泄漏。

use super::constants::OMITTED_LINE_COUNT_LIMIT;
use crate::tui::render::output::blocks::edit_diff::render_edit_diff;
use crate::tui::render::output::primitives::wrap::{wrap_spans_with_prefix, WrapMode};
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::{result_policy, ResultPolicy, ResultRender};
use crate::tui::render::theme;
use crate::tui::view_model::output::{
    AgentActivityKindView, AgentActivityLineView, ToolResultBlockView,
};
use ratatui::style::Style;
use ratatui::text::Span;
use serde_json::Value;
use std::rc::Rc;

/// 从结构化 JSON content 中提取显示文本。
/// 优先级：display > message > text > 序列化 JSON
fn display_text_from_json(content: &Value) -> Option<String> {
    if let Some(display) = content.get("display").and_then(|v| v.as_str()) {
        return Some(display.to_string());
    }
    if let Some(message) = content.get("message").and_then(|v| v.as_str()) {
        return Some(message.to_string());
    }
    if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }
    None
}

/// 尝试将 result_text 解析为结构化 JSON 并提取显示文本。
/// 如果解析失败或没有合适的字段，返回原始 result_text。
fn resolve_display_text(result_text: &str) -> String {
    // 尝试解析为 JSON
    if let Ok(content) = serde_json::from_str::<Value>(result_text) {
        if let Some(display) = display_text_from_json(&content) {
            return display;
        }
    }
    result_text.to_string()
}

pub fn render_tool_result(
    block_id: &str,
    view: &ToolResultBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    let policy = result_policy(&view.tool_title);
    // 解析结构化 JSON，提取显示文本
    let display_text = resolve_display_text(&view.result_text);
    crate::tui::log_debug!(
        "render tool_result block_id={} tool_title={} result_len={} display_len={} width={} style={:?} policy={:?}",
        block_id,
        view.tool_title,
        view.result_text.len(),
        display_text.len(),
        ctx.text_width,
        view.style,
        policy,
    );

    let lines = match policy {
        ResultPolicy::Hidden => vec![],
        ResultPolicy::Visible { .. } if view.activity_lines.is_some() => render_activity_lines(
            view.activity_lines.as_deref().unwrap_or_default(),
            view.workspace_root.as_deref(),
            ctx.text_width.into(),
        ),
        ResultPolicy::Visible {
            max_lines,
            render_kind,
            tail_mode,
        } => {
            let limit = max_lines.unwrap_or(usize::MAX);
            match render_kind {
                ResultRender::Diff => render_edit_diff(
                    view.data.as_ref(),
                    view.args_preview.as_deref(),
                    &display_text,
                    ctx.text_width,
                )
                .unwrap_or_else(|| {
                    format_result_lines(&view.tool_title, &display_text, ctx.text_width, limit)
                }),
                ResultRender::Plain => {
                    if tail_mode {
                        format_result_lines_tail(
                            &view.tool_title,
                            &display_text,
                            ctx.text_width,
                            limit,
                        )
                    } else {
                        format_result_lines(&view.tool_title, &display_text, ctx.text_width, limit)
                    }
                }
            }
        }
    };

    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

fn render_activity_lines(
    activities: &[AgentActivityLineView],
    workspace_root: Option<&std::path::Path>,
    width: usize,
) -> Vec<RenderedLine> {
    let activity_style = Style::default().fg(theme::TEXT_DIM);
    let marker_style = Style::default().fg(theme::TEXT_MUTED);
    activities
        .iter()
        .flat_map(|activity| {
            let mut spans = Vec::new();
            if activity.kind == AgentActivityKindView::ToolCall {
                spans.push(Span::styled("→ ", marker_style));
            }
            let content = match &activity.content {
                crate::tui::view_model::output::AgentActivityContentView::Text(content) => {
                    content.clone()
                }
                crate::tui::view_model::output::AgentActivityContentView::ToolCall {
                    name,
                    input,
                } => crate::tui::render::output::tool_display::format_subagent_tool_header(
                    name,
                    input,
                    workspace_root,
                ),
            };
            spans.push(Span::styled(content, activity_style));
            wrap_spans_with_prefix(spans, width, None, WrapMode::Word)
                .into_iter()
                .map(|line| line.with_style(activity_style))
        })
        .collect()
}

/// 渲染 Plain 工具结果：**纯文本原样**逐行，按 `max_lines` 截断。
///
/// 用暗色（`theme::TEXT_DIM`）——文件/命令输出预览不跟随 tool 状态色（状态绿/红只在 header
/// 的 ✓/✗ marker）；**不做 markdown 重渲染**——避免文件内容里的 markdown（表格/标题/fence）
/// 被渲染变形，保留原文（含 Read 行号/缩进，#91）。
fn format_result_lines(
    _tool_name: &str,
    result: &str,
    width: u16,
    max_lines: usize,
) -> Vec<RenderedLine> {
    if result.trim().is_empty() {
        return Vec::new();
    }
    if max_lines == 0 {
        return Vec::new();
    }
    let base = Style::default().fg(theme::TEXT_DIM);
    let mut iter = result.lines();
    let mut out: Vec<RenderedLine> = Vec::new();
    // 逐原始行处理，每行 wrap 后累计渲染行数，达到 max_lines 即停止
    while out.len() < max_lines {
        match iter.next() {
            None => break,
            Some(line) => {
                out.extend(wrap_spans_with_prefix(
                    vec![Span::styled(line.to_string(), base)],
                    width as usize,
                    None,
                    WrapMode::Word,
                ));
            }
        }
    }
    // 截断 wrap 展开后超出 max_lines 的渲染行（处理单行长输出场景）
    if out.len() > max_lines {
        out.truncate(max_lines);
    }
    let omitted = iter.by_ref().take(OMITTED_LINE_COUNT_LIMIT + 1).count();
    if omitted > 0 {
        let omitted_label = if omitted > OMITTED_LINE_COUNT_LIMIT {
            format!("{OMITTED_LINE_COUNT_LIMIT}+")
        } else {
            omitted.to_string()
        };
        out.push(RenderedLine::new(vec![Span::styled(
            format!("... ({omitted_label} lines omitted)"),
            base,
        )]));
    }
    out
}

/// 渲染 Plain 工具结果（tail 模式）：只显示最后 `max_lines` 行。
/// 适用于 Bash 等持续输出的工具，用户关注最新输出。
fn format_result_lines_tail(
    _tool_name: &str,
    result: &str,
    width: u16,
    max_lines: usize,
) -> Vec<RenderedLine> {
    if result.trim().is_empty() {
        return Vec::new();
    }
    if max_lines == 0 {
        return Vec::new();
    }
    let base = Style::default().fg(theme::TEXT_DIM);
    let all_lines: Vec<&str> = result.lines().collect();
    let start = all_lines.len().saturating_sub(max_lines);
    let mut out: Vec<RenderedLine> = Vec::new();
    // get(start..) 返回 Option，不会 panic
    if let Some(remaining) = all_lines.get(start..) {
        for line in remaining {
            out.extend(wrap_spans_with_prefix(
                vec![Span::styled(line.to_string(), base)],
                width as usize,
                None,
                WrapMode::Word,
            ));
        }
    }
    // 截断 wrap 展开后超出 max_lines 的渲染行
    if out.len() > max_lines {
        out.truncate(max_lines);
    }
    // 显示省略的行数
    if start > 0 {
        out.insert(
            0,
            RenderedLine::new(vec![Span::styled(
                format!("... ({start} lines above)"),
                base,
            )]),
        );
    }
    out
}

#[cfg(test)]
#[path = "tool_result_tests.rs"]
mod tests;
