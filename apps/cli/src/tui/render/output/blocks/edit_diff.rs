//! Edit 工具结果的 diff 渲染。
//!
//! **数据来源（两条路径，结构化优先）**：
//! 1. `edit_diff_from_data`：从 `EditResult` 的结构化 JSON（`old`/`new`/`start_line`）
//!    直接构造 `EditDiff`——这是 #546 后的正道，diff 内容走 `data` 通道而非 LLM `text`。
//! 2. `parse_edit_diff`：从 `text` 中的 `---DIFF:LINE:N---` 标记解析——历史 session 兼容
//!    （旧 session 的 `data` 里没有 `old`/`new`/`start_line`，只能从 text 回退解析）。
//!
//! 复用 `primitives::diff::diff`（行号 + 加减语义色 + 语法高亮 + 缩进）渲染为
//! `RenderedLine`，下游统一经 `apply_selection_overlay` 可选中并保留前景色（bug #61）。

use super::constants::{
    DIFF_LINE_PREFIX, DIFF_MARKER_PREFIX, DIFF_MARKER_SUFFIX, HIGHLIGHT_MAX_LINE_BYTES,
    HIGHLIGHT_MAX_SIDE_LINES, HIGHLIGHT_MAX_TOTAL_BYTES, LEGACY_DIFF_MARKER, RENDER_MAX_LINE_BYTES,
    RENDER_MAX_SIDE_LINES, RENDER_MAX_TOTAL_BYTES, RETAINED_LINES_PER_END,
};
use crate::tui::render::output::primitives::diff::diff_from;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::syntax::extension_from_path;
use crate::tui::render::theme;
use ratatui::style::Style;
use ratatui::text::Span;
use serde_json::Value;

/// Edit 工具结果中包裹 old/new 文本的旧标记。

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffRenderMode {
    Highlighted,
    Plain,
    HeadTailPlain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DiffRenderBudget {
    mode: DiffRenderMode,
    old_lines: usize,
    new_lines: usize,
    total_bytes: usize,
    max_line_bytes: usize,
}

impl DiffRenderBudget {
    fn classify(old: &str, new: &str) -> Self {
        let old_stats = source_stats(old);
        let new_stats = source_stats(new);
        let old_lines = old_stats.line_count;
        let new_lines = new_stats.line_count;
        let total_bytes = old.len().saturating_add(new.len());
        let max_line_bytes = old_stats.max_line_bytes.max(new_stats.max_line_bytes);
        let mode = if old_lines > RENDER_MAX_SIDE_LINES
            || new_lines > RENDER_MAX_SIDE_LINES
            || total_bytes > RENDER_MAX_TOTAL_BYTES
            || max_line_bytes > RENDER_MAX_LINE_BYTES
        {
            DiffRenderMode::HeadTailPlain
        } else if old_lines > HIGHLIGHT_MAX_SIDE_LINES
            || new_lines > HIGHLIGHT_MAX_SIDE_LINES
            || total_bytes > HIGHLIGHT_MAX_TOTAL_BYTES
            || max_line_bytes > HIGHLIGHT_MAX_LINE_BYTES
        {
            DiffRenderMode::Plain
        } else {
            DiffRenderMode::Highlighted
        };
        Self {
            mode,
            old_lines,
            new_lines,
            total_bytes,
            max_line_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SourceStats {
    line_count: usize,
    max_line_bytes: usize,
}

fn source_stats(source: &str) -> SourceStats {
    source
        .lines()
        .fold(SourceStats::default(), |mut stats, line| {
            stats.line_count += 1;
            stats.max_line_bytes = stats.max_line_bytes.max(line.len());
            stats
        })
}

struct SourceWindow {
    head: String,
    tail: String,
    tail_start: usize,
    omitted_lines: usize,
}

fn source_window(source: &str, line_count: usize) -> SourceWindow {
    let head_count = line_count.min(RETAINED_LINES_PER_END);
    let tail_count = line_count
        .saturating_sub(head_count)
        .min(RETAINED_LINES_PER_END);
    let tail_start_index = line_count.saturating_sub(tail_count);
    let head = source
        .lines()
        .take(head_count)
        .map(truncate_render_line)
        .collect::<Vec<_>>()
        .join("\n");
    let tail = source
        .lines()
        .skip(tail_start_index)
        .map(truncate_render_line)
        .collect::<Vec<_>>()
        .join("\n");
    SourceWindow {
        head,
        tail,
        tail_start: tail_start_index.saturating_add(1),
        omitted_lines: line_count.saturating_sub(head_count.saturating_add(tail_count)),
    }
}

fn truncate_render_line(line: &str) -> String {
    if line.len() <= RENDER_MAX_LINE_BYTES {
        return line.to_string();
    }
    const LABEL_RESERVE_BYTES: usize = 128;
    let prefix = crate::tui::text::safe_byte_prefix(
        line,
        RENDER_MAX_LINE_BYTES.saturating_sub(LABEL_RESERVE_BYTES),
    );
    format!("{prefix} …（单行已截断，原始 {} 字节）", line.len())
}

fn render_plain(parsed: &EditDiff, budget: DiffRenderBudget, width: u16) -> Vec<RenderedLine> {
    if budget.max_line_bytes <= RENDER_MAX_LINE_BYTES {
        return diff_from(
            &parsed.old,
            &parsed.new,
            parsed.start_line,
            parsed.start_line,
            None,
            width,
        );
    }
    let old = parsed
        .old
        .lines()
        .map(truncate_render_line)
        .collect::<Vec<_>>()
        .join("\n");
    let new = parsed
        .new
        .lines()
        .map(truncate_render_line)
        .collect::<Vec<_>>()
        .join("\n");
    diff_from(
        &old,
        &new,
        parsed.start_line,
        parsed.start_line,
        None,
        width,
    )
}

fn render_head_tail_plain(
    parsed: &EditDiff,
    budget: DiffRenderBudget,
    width: u16,
) -> Vec<RenderedLine> {
    let old = source_window(&parsed.old, budget.old_lines);
    let new = source_window(&parsed.new, budget.new_lines);
    let mut lines = diff_from(
        &old.head,
        &new.head,
        parsed.start_line,
        parsed.start_line,
        None,
        width,
    );
    if old.omitted_lines > 0 || new.omitted_lines > 0 {
        let omitted = format!(
            "─── Edit diff 中间已省略（old {} 行 / new {} 行）───",
            old.omitted_lines, new.omitted_lines
        );
        lines.push(RenderedLine::new(vec![Span::styled(
            omitted,
            Style::default().fg(theme::TEXT_DIM),
        )]));
    }
    lines.extend(diff_from(
        &old.tail,
        &new.tail,
        parsed
            .start_line
            .saturating_add(old.tail_start.saturating_sub(1)),
        parsed
            .start_line
            .saturating_add(new.tail_start.saturating_sub(1)),
        None,
        width,
    ));
    lines
}

/// 解析后的 Edit diff 数据：变更前/后文本与真实文件起始行号。
pub struct EditDiff {
    pub old: String,
    pub new: String,
    pub start_line: usize,
}

/// 从 Edit 工具结果文本中解析出 old/new 两份文本。
///
/// 期望格式：
/// ```text
/// replaced N occurrence(s) in {path}
/// ---DIFF:LINE:{start_line}---
/// {old}
/// ---DIFF:LINE:{start_line}---
/// {new}
/// ```
/// 兼容旧格式 `---DIFF---`，旧格式起始行号默认为 1。
pub fn parse_edit_diff(result: &str) -> Option<EditDiff> {
    let first = find_diff_marker(result)?;
    let after_first = first.end;
    let second = find_diff_marker(result.get(after_first..)?)?;
    let second_start = after_first + second.start;
    let second_end = after_first + second.end;

    Some(EditDiff {
        old: strip_edge_newlines(result.get(after_first..second_start)?).to_string(),
        new: strip_edge_newlines(result.get(second_end..)?).to_string(),
        start_line: first.start_line,
    })
}

struct DiffMarker {
    start: usize,
    end: usize,
    start_line: usize,
}

fn find_diff_marker(text: &str) -> Option<DiffMarker> {
    let start = text.find(DIFF_MARKER_PREFIX)?;
    let tail = text.get(start + DIFF_MARKER_PREFIX.len()..)?;
    let suffix_start = tail.find(DIFF_MARKER_SUFFIX)?;
    let relative_end = DIFF_MARKER_PREFIX.len() + suffix_start + DIFF_MARKER_SUFFIX.len();
    let marker = text.get(start..start + relative_end)?;
    let start_line = parse_diff_marker_start_line(marker)?;
    Some(DiffMarker {
        start,
        end: start + relative_end,
        start_line,
    })
}

fn parse_diff_marker_start_line(marker: &str) -> Option<usize> {
    if marker == LEGACY_DIFF_MARKER {
        return Some(1);
    }
    let line = marker
        .strip_prefix(DIFF_MARKER_PREFIX)?
        .strip_suffix(DIFF_MARKER_SUFFIX)?
        .strip_prefix(DIFF_LINE_PREFIX)?
        .parse::<usize>()
        .ok()?;
    Some(line.max(1))
}

/// 去除标记前后插入的单个换行符，保留内部内容原样。
fn strip_edge_newlines(text: &str) -> &str {
    let text = text.strip_prefix('\n').unwrap_or(text);
    text.strip_suffix('\n').unwrap_or(text)
}

/// 从结构化 data（`EditResult` JSON）直接构造 `EditDiff`（#546）。
///
/// 这是优先路径：diff 内容走 `data` 通道而非 LLM `text`。
/// 返回 `None` 时调用方应回退到 `parse_edit_diff`（兼容历史 session）。
pub fn edit_diff_from_data(data: Option<&Value>) -> Option<EditDiff> {
    let data = data?;
    let old = data.get("old")?.as_str()?;
    let new = data.get("new")?.as_str()?;
    let start_line = data.get("start_line")?.as_u64()?.max(1) as usize;
    Some(EditDiff {
        old: old.to_string(),
        new: new.to_string(),
        start_line,
    })
}

/// 推断 Edit diff 的语法高亮扩展名。
///
/// 运行时 `view.title` 是裸工具名 `"Edit"`（无路径括号，见
/// `view_assembler/output.rs` 的 `title: call.name.clone()`），故 **不可**从 title 取。
/// 优先级：
/// 1. `summary`（工具入参 JSON，含 `file_path`，见 `adapter` 将 `input.to_string()`
///    存入 summary）。
/// 2. 退而从 Edit 结果 header 的 `in {path}` 解析（`agent/tools/src/file_edit.rs`
///    输出 `replaced N occurrence(s)[...] in {file_path}`）。
pub fn file_ext_for_edit(summary: Option<&str>, result: &str) -> Option<String> {
    file_ext_from_args(summary).or_else(|| file_ext_from_result_header(result))
}

/// 从工具入参 JSON 中取 `file_path` 的扩展名。
fn file_ext_from_args(summary: Option<&str>) -> Option<String> {
    let summary = summary?;
    let value: serde_json::Value = serde_json::from_str(summary).ok()?;
    let path = value.get("file_path")?.as_str()?;
    extension_from_path(path).map(str::to_string)
}

/// 从 Edit 结果 header 的 `in {path}` 子串解析扩展名。
fn file_ext_from_result_header(result: &str) -> Option<String> {
    // 仅取首行 header（DIFF 正文不含 "in " 路径语义，避免误判）。
    let header = result.lines().next()?;
    let path = header.rsplit_once(" in ")?.1.trim();
    extension_from_path(path).map(str::to_string)
}

/// 若 result 是 Edit diff，则渲染为带行号/语义色/语法高亮的 diff 行。
///
/// **数据来源优先级**（#546）：
/// 1. `data`（结构化 `EditResult` JSON）→ `edit_diff_from_data`
/// 2. `result`（text 中的 `---DIFF---` 标记）→ `parse_edit_diff`（历史兼容）
///
/// `summary`（工具入参 JSON）用于推断语法高亮语言；退而用 result header 的 `in {path}`。
/// `width` 传入 diff 原语。
pub fn render_edit_diff(
    data: Option<&Value>,
    summary: Option<&str>,
    result: &str,
    width: u16,
) -> Option<Vec<RenderedLine>> {
    let parsed = edit_diff_from_data(data).or_else(|| parse_edit_diff(result))?;
    #[cfg(test)]
    let started = std::time::Instant::now();
    let ext = file_ext_for_edit(summary, result);
    let budget = DiffRenderBudget::classify(&parsed.old, &parsed.new);
    if budget.mode != DiffRenderMode::Highlighted {
        crate::tui::log_debug!(
            "Edit diff 渲染降级 mode={:?} old_lines={} new_lines={} total_bytes={} max_line_bytes={}",
            budget.mode,
            budget.old_lines,
            budget.new_lines,
            budget.total_bytes,
            budget.max_line_bytes,
        );
    }
    let lines = match budget.mode {
        DiffRenderMode::Highlighted => diff_from(
            &parsed.old,
            &parsed.new,
            parsed.start_line,
            parsed.start_line,
            ext.as_deref(),
            width,
        ),
        DiffRenderMode::Plain => render_plain(&parsed, budget, width),
        DiffRenderMode::HeadTailPlain => render_head_tail_plain(&parsed, budget, width),
    };
    #[cfg(test)]
    crate::tui::render::performance::record_edit_diff(started.elapsed());
    Some(lines)
}

#[cfg(test)]
#[path = "edit_diff_tests.rs"]
mod tests;
