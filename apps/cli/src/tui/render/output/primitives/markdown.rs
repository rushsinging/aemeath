//! markdown 原语：解析 inline markdown -> 显示 spans，按宽度换行；plain 去标记。
//!
//! 在 inline 之上补齐两类常见块级装饰（按行检测，与 fenced 逐行喂入契合）：
//! - 引用块：`> ` / 嵌套 `> > ` 前缀渲染为弱化色竖线 `│ `，正文仍走 inline markdown。
//! - 列表项：`- ` / `* ` / `+ ` 无序、`N. ` 有序，保留缩进层级，标记着强调色，
//!   正文（含 bold/code/link）仍走 inline markdown。

use super::constants::{BULLET, QUOTE_BAR};
use crate::tui::render::output::markdown as md;
use crate::tui::render::output::primitives::wrap::{wrap_spans_with_prefix, WrapMode};
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::theme;
use crate::tui::text::split_at_ascii;
use ratatui::style::Style;
use ratatui::text::Span;

pub fn markdown(text: &str, base_style: Style, width: u16) -> Vec<RenderedLine> {
    // 空文本（包括无换行符的空串）仍产出一行，保持与历史行为一致。
    if text.is_empty() {
        return inline_lines("", base_style, width);
    }
    // 单块内部逐行渲染；块间空白由 fenced orchestration 统一负责。
    text.lines()
        .flat_map(|line| render_line(line, base_style, width))
        .collect()
}

/// 渲染单行：先识别块级前缀（引用 / 列表），剥离后正文走 inline，再把
/// 前缀以样式化 marker 拼回，并保持 `plain` 与可见 spans 一致。
fn render_line(line: &str, base_style: Style, width: u16) -> Vec<RenderedLine> {
    if let Some((bars, body)) = strip_blockquote(line) {
        let marker_plain = QUOTE_BAR.repeat(bars);
        let marker_width = marker_plain.chars().count() as u16;
        let inner_width = width.saturating_sub(marker_width).max(1);
        let marker_style = Style::default().fg(theme::TEXT_DIM);
        let body_style = base_style.fg(theme::TEXT_MUTED);
        return inline_lines(body, body_style, inner_width)
            .into_iter()
            .map(|line| prepend_marker(&marker_plain, marker_style, line))
            .collect();
    }

    if let Some((indent, marker, body)) = strip_list_item(line) {
        let marker_plain = format!("{indent}{marker}");
        let marker_width = marker_plain.chars().count() as u16;
        let inner_width = width.saturating_sub(marker_width).max(1);
        let marker_style = base_style.fg(theme::ACCENT);
        return inline_lines(body, base_style, inner_width)
            .into_iter()
            .enumerate()
            .map(|(idx, line)| {
                if idx == 0 {
                    prepend_marker(&marker_plain, marker_style, line)
                } else {
                    // 续行按 marker 宽度缩进对齐，不重复 marker。
                    let pad = " ".repeat(marker_plain.chars().count());
                    prepend_marker(&pad, base_style, line)
                }
            })
            .collect();
    }

    inline_lines(line, base_style, width)
}

/// 普通 inline markdown 行（无块级前缀）。
fn inline_lines(text: &str, base_style: Style, width: u16) -> Vec<RenderedLine> {
    let (spans, links) = md::inline_markdown_spans_with_links(text, base_style);
    let wrapped = wrap_spans_with_prefix(spans, width as usize, None, WrapMode::Word);
    distribute_links(wrapped, links)
}

/// 将原始行内 link 偏移分配到 wrap 后各行。
fn distribute_links(
    mut wrapped: Vec<RenderedLine>,
    links: Vec<crate::tui::render::output::rendered::LinkSpan>,
) -> Vec<RenderedLine> {
    if links.is_empty() {
        return wrapped;
    }

    let mut offset = 0usize;
    for line in &mut wrapped {
        let line_len = line.plain.chars().count();
        let line_start = offset;
        let line_end = offset + line_len;

        let line_links: Vec<_> = links
            .iter()
            .filter(|ls| ls.col_start >= line_start && ls.col_start < line_end)
            .cloned()
            .map(|mut ls| {
                ls.col_start -= line_start;
                ls.col_end = (ls.col_end - line_start).min(line_len);
                ls
            })
            .collect();
        line.links = line_links;
        offset = line_end;
    }
    wrapped
}

/// 在一行已渲染产物前补一个样式化 marker，保持 plain 与 spans 一致。
/// marker 宽度补偿到 links 的 col_start（marker 不参与 plain 偏移计算——gutter 机制已处理）。
fn prepend_marker(marker_plain: &str, marker_style: Style, line: RenderedLine) -> RenderedLine {
    let mut spans = vec![Span::styled(marker_plain.to_string(), marker_style)];
    spans.extend(line.spans);
    let plain = format!("{marker_plain}{}", line.plain);
    // links 的 col_start 是基于原 plain（不含 marker）的偏移；
    // marker 加入后 plain 多了 marker 前缀，但 gutter_cols 机制会补偿显示列差。
    // 此处保持 links 偏移不变（仍基于 content 部分）。
    RenderedLine::with_plain_and_links(spans, plain, line.links)
}

/// 识别引用块前缀，返回（嵌套层数, 去前缀后的正文）。
/// 仅当行以可选空白 + `>` 开头时成立；逐层吃掉 `>` 及其后单个空格。
fn strip_blockquote(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('>') {
        return None;
    }
    let mut rest = trimmed;
    let mut bars = 0usize;
    while let Some(after) = rest.strip_prefix('>') {
        bars += 1;
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    Some((bars, rest))
}

/// 识别列表项前缀，返回（缩进, 渲染用 marker, 正文）。
/// - 无序：`- ` / `* ` / `+ ` → 统一 `BULLET`。
/// - 有序：`N. ` / `N) ` → 原样保留 `N. `。
fn strip_list_item(line: &str) -> Option<(String, String, &str)> {
    let (indent, rest) = split_at_ascii(line, |c| c.is_ascii_whitespace());
    if let Some(body) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        return Some((indent.to_string(), BULLET.to_string(), body));
    }
    // 有序列表：开头若干数字 + `.`/`)` + 空格。
    let (digits, after) = split_at_ascii(rest, |c| c.is_ascii_digit());
    if !digits.is_empty() {
        for sep in [". ", ") "] {
            if let Some(body) = after.strip_prefix(sep) {
                return Some((indent.to_string(), format!("{digits}. "), body));
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;
