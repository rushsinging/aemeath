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

use crate::tui::render::output::primitives::diff::diff_from;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::syntax::extension_from_path;
use crate::tui::render::theme;
use ratatui::style::Style;
use ratatui::text::Span;
use serde_json::Value;

/// Edit 工具结果中包裹 old/new 文本的旧标记。
pub(crate) const LEGACY_DIFF_MARKER: &str = "---DIFF---";
const DIFF_MARKER_PREFIX: &str = "---DIFF";
const DIFF_MARKER_SUFFIX: &str = "---";
const DIFF_LINE_PREFIX: &str = ":LINE:";

const HIGHLIGHT_MAX_SIDE_LINES: usize = 20_000;
const HIGHLIGHT_MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const HIGHLIGHT_MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
const RENDER_MAX_SIDE_LINES: usize = 100_000;
const RENDER_MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const RENDER_MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const RETAINED_LINES_PER_END: usize = 250;

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
mod tests {
    use super::*;

    fn edit_result(old: &str, new: &str) -> String {
        format!("replaced 1 occurrence(s) in src/lib.rs\n---DIFF---\n{old}\n---DIFF---\n{new}")
    }

    #[test]
    fn test_parse_edit_diff_extracts_old_and_new() {
        let parsed = parse_edit_diff(&edit_result("let a = 1;", "let a = 2;")).unwrap();

        assert_eq!(parsed.old, "let a = 1;");
        assert_eq!(parsed.new, "let a = 2;");
    }

    #[test]
    fn test_parse_edit_diff_extracts_real_start_line() {
        let result = "replaced 1 occurrence(s) in src/lib.rs\n---DIFF:LINE:42---\nold\n---DIFF:LINE:42---\nnew";
        let parsed = parse_edit_diff(result).unwrap();

        assert_eq!(parsed.old, "old");
        assert_eq!(parsed.new, "new");
        assert_eq!(parsed.start_line, 42);
    }

    #[test]
    fn test_parse_edit_diff_legacy_marker_defaults_to_line_one() {
        let parsed = parse_edit_diff(&edit_result("old", "new")).unwrap();

        assert_eq!(parsed.start_line, 1);
    }

    #[test]
    fn test_parse_edit_diff_multiline_preserves_inner_content() {
        let old = "fn f() {\n    1\n}";
        let new = "fn f() {\n    2\n}";
        let parsed = parse_edit_diff(&edit_result(old, new)).unwrap();

        assert_eq!(parsed.old, old);
        assert_eq!(parsed.new, new);
    }

    #[test]
    fn test_parse_edit_diff_returns_none_without_marker() {
        assert!(parse_edit_diff("wrote 10 bytes to a.txt").is_none());
        assert!(parse_edit_diff("done: 3 matches").is_none());
    }

    #[test]
    fn test_file_ext_for_edit_from_args_json() {
        // 正常路径：summary 是入参 JSON，含 file_path → 取扩展名。
        let summary = r#"{"file_path":"src/lib.rs","old_string":"a","new_string":"b"}"#;
        assert_eq!(
            file_ext_for_edit(Some(summary), "replaced 1 occurrence(s) in src/lib.rs").as_deref(),
            Some("rs")
        );
    }

    #[test]
    fn test_file_ext_for_edit_falls_back_to_result_header() {
        // summary 缺失/无 file_path → 从结果 header 的 "in {path}" 解析。
        let result = "replaced 2 occurrence(s) in /a/b/main.py\n---DIFF---\nx\n---DIFF---\ny";
        assert_eq!(file_ext_for_edit(None, result).as_deref(), Some("py"));
        assert_eq!(file_ext_for_edit(Some("{}"), result).as_deref(), Some("py"));
    }

    #[test]
    fn test_file_ext_for_edit_none_when_no_extension_or_no_source() {
        // 边界/错误：无扩展名、无 in 路径、非 JSON summary 均返回 None。
        assert!(file_ext_for_edit(Some("not json"), "no path here").is_none());
        assert!(file_ext_for_edit(Some(r#"{"file_path":"Makefile"}"#), "done").is_none());
        assert!(file_ext_for_edit(None, "replaced 1 occurrence(s) in Dockerfile").is_none());
    }

    #[test]
    fn test_render_edit_diff_emits_line_numbers_signs_indent_and_color() {
        let result = edit_result("let a = 1;", "let a = 2;");
        let summary = r#"{"file_path":"src/lib.rs"}"#;
        let lines = render_edit_diff(None, Some(summary), &result, 80).unwrap();

        let plains: Vec<&str> = lines.iter().map(|line| line.plain.as_str()).collect();

        // 删除行带 "- " 与原文本，新增行带 "+ " 与新文本（加减语义）。
        assert!(
            plains.iter().any(|p| p.contains("- ") && p.contains("1;")),
            "应含删除行，got: {plains:?}"
        );
        assert!(
            plains.iter().any(|p| p.contains("+ ") && p.contains("2;")),
            "应含新增行，got: {plains:?}"
        );
        // 块缩进由 gutter 注入（#60/#63）：diff 行不再自拼行首 INDENT，删除行从行号区起。
        let del = lines
            .iter()
            .find(|line| line.plain.contains("- ") && line.plain.contains("1;"))
            .expect("删除行存在");
        assert!(
            !del.plain.starts_with("  "),
            "删除行不应自拼行首块缩进，got: {:?}",
            del.plain
        );
        // 至少一行带前景色 span（语义色 / 语法高亮）。
        assert!(
            lines
                .iter()
                .any(|line| line.spans.iter().any(|span| span.style.fg.is_some())),
            "应有带前景色的 span"
        );
    }

    #[test]
    fn test_render_edit_diff_none_for_non_diff_result() {
        assert!(render_edit_diff(None, Some(r#"{"file_path":"a.rs"}"#), "120 lines", 80).is_none());
    }

    #[test]
    fn test_render_edit_diff_does_not_contain_raw_marker() {
        let result = edit_result("a", "b");
        let lines = render_edit_diff(None, Some(r#"{"file_path":"x.rs"}"#), &result, 80).unwrap();

        assert!(
            lines
                .iter()
                .all(|line| !line.plain.contains(LEGACY_DIFF_MARKER)),
            "渲染后不应残留原始标记"
        );
    }

    #[test]
    fn test_render_edit_diff_real_bare_title_summary_drives_syntax_highlight() {
        // M1 回归：运行时 title 是裸 "Edit"（无括号路径），ext 必须从 summary 的
        // file_path 推断。注入真实 summary，断言 Rust 语法高亮被激活
        //（新增行因高亮产生 >2 个 span，而非单色 1 个内容 span）。
        // header 无可解析扩展名（Dockerfile），确保基线不会经 header 回退拿到 ext。
        let result =
            "edited Dockerfile\n---DIFF---\nfn old() {}\n---DIFF---\nfn new() {}".to_string();
        let summary = r#"{"file_path":"src/lib.rs","old_string":"fn old() {}"}"#;

        let with_ext = render_edit_diff(None, Some(summary), &result, 80).unwrap();
        let without_ext = render_edit_diff(None, Some("{}"), &result, 80).unwrap();

        // 新增行（含 "new"）。
        let added_with = with_ext
            .iter()
            .find(|l| l.plain.contains("new"))
            .expect("新增行存在");
        let added_without = without_ext
            .iter()
            .find(|l| l.plain.contains("new"))
            .expect("新增行存在");

        // 有 ext → 语法高亮产生更多 span；无 ext → 单色少 span。
        assert!(
            added_with.spans.len() > added_without.spans.len(),
            "summary 含 file_path 时应激活语法高亮（更多 span）: with={} without={}",
            added_with.spans.len(),
            added_without.spans.len()
        );
    }

    // ── #546：结构化 data 通道测试 ──────────────────────────────────

    #[test]
    fn test_edit_diff_from_data_extracts_structured_fields() {
        let data = serde_json::json!({
            "file_path": "src/lib.rs",
            "replacements_made": 1,
            "dry_run": false,
            "old": "let a = 1;",
            "new": "let a = 2;",
            "start_line": 5
        });
        let parsed = edit_diff_from_data(Some(&data)).unwrap();

        assert_eq!(parsed.old, "let a = 1;");
        assert_eq!(parsed.new, "let a = 2;");
        assert_eq!(parsed.start_line, 5);
    }

    #[test]
    fn test_edit_diff_from_data_returns_none_when_missing_fields() {
        // 缺 old/new/start_line → None（回退到 parse_edit_diff）
        let data = serde_json::json!({"file_path": "a.rs", "replacements_made": 1});
        assert!(edit_diff_from_data(Some(&data)).is_none());
        assert!(edit_diff_from_data(None).is_none());
    }

    #[test]
    fn test_render_edit_diff_prefers_data_over_text() {
        // data 含结构化 diff 时优先走 data，即使 text 不含 ---DIFF--- 标记也能渲染。
        let data = serde_json::json!({
            "old": "let a = 1;",
            "new": "let a = 2;",
            "start_line": 1
        });
        let summary = r#"{"file_path":"src/lib.rs"}"#;
        let lines = render_edit_diff(Some(&data), Some(summary), "Replaced 1 occurrence(s)", 80)
            .expect("data 通道应成功渲染 diff");

        let plains: Vec<&str> = lines.iter().map(|line| line.plain.as_str()).collect();
        assert!(
            plains.iter().any(|p| p.contains("- ") && p.contains("1;")),
            "应含删除行，got: {plains:?}"
        );
        assert!(
            plains.iter().any(|p| p.contains("+ ") && p.contains("2;")),
            "应含新增行，got: {plains:?}"
        );
    }

    #[test]
    fn test_render_edit_diff_falls_back_to_text_when_data_absent() {
        // data 为 None（历史 session）时回退到 parse_edit_diff。
        let result = edit_result("let a = 1;", "let a = 2;");
        let lines = render_edit_diff(None, Some(r#"{"file_path":"src/lib.rs"}"#), &result, 80)
            .expect("回退 parse_edit_diff 应成功渲染");

        assert!(
            lines.iter().any(|l| l.plain.contains("1;")),
            "回退路径也应正确渲染 diff"
        );
    }

    fn numbered_source(lines: usize, changed: bool) -> String {
        (0..lines)
            .map(|index| {
                if changed && index == lines / 2 {
                    format!("fn item_{index}() {{ new_value(); }}")
                } else {
                    format!("fn item_{index}() {{ old_value(); }}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn render_budget_uses_measured_threshold_boundaries() {
        let highlighted = numbered_source(20_000, false);
        let plain = numbered_source(20_001, false);
        let extreme = numbered_source(100_001, false);

        assert_eq!(
            DiffRenderBudget::classify(&highlighted, &highlighted).mode,
            DiffRenderMode::Highlighted
        );
        assert_eq!(
            DiffRenderBudget::classify(&plain, &plain).mode,
            DiffRenderMode::Plain
        );
        assert_eq!(
            DiffRenderBudget::classify(&extreme, &extreme).mode,
            DiffRenderMode::HeadTailPlain
        );
    }

    #[test]
    fn edit_within_highlight_budget_keeps_syntax_highlighting() {
        let old = numbered_source(20, false);
        let new = numbered_source(20, true);
        let data = serde_json::json!({"old": old, "new": new, "start_line": 100});

        let (lines, snapshot) = crate::tui::render::performance::capture(|| {
            render_edit_diff(
                Some(&data),
                Some(r#"{"file_path":"src/lib.rs"}"#),
                "edited src/lib.rs",
                80,
            )
            .unwrap()
        });

        assert!(snapshot.syntax_highlight_calls > 0);
        assert!(lines.iter().all(|line| !line.plain.contains("省略")));
    }

    #[test]
    fn edit_over_highlight_line_budget_keeps_full_diff_without_syntax_highlighting() {
        let old = numbered_source(20_001, false);
        let new = numbered_source(20_001, true);
        let data = serde_json::json!({"old": old, "new": new, "start_line": 1});

        let (lines, snapshot) = crate::tui::render::performance::capture(|| {
            render_edit_diff(
                Some(&data),
                Some(r#"{"file_path":"src/lib.rs"}"#),
                "edited src/lib.rs",
                80,
            )
            .unwrap()
        });

        assert_eq!(snapshot.syntax_highlight_calls, 0);
        assert_eq!(lines.len(), 20_002);
        assert!(lines.iter().all(|line| !line.plain.contains("省略")));
        assert!(lines.iter().any(|line| line.plain.contains("+ ")));
        assert!(lines.iter().any(|line| line.plain.contains("- ")));
    }

    #[test]
    fn extreme_edit_keeps_head_and_tail_with_real_line_numbers() {
        let old = numbered_source(100_001, false);
        let new = numbered_source(100_001, true);
        let data = serde_json::json!({"old": old, "new": new, "start_line": 42});

        let (lines, snapshot) = crate::tui::render::performance::capture(|| {
            render_edit_diff(
                Some(&data),
                Some(r#"{"file_path":"src/lib.rs"}"#),
                "edited src/lib.rs",
                80,
            )
            .unwrap()
        });

        assert_eq!(snapshot.syntax_highlight_calls, 0);
        assert!(lines.len() <= 503, "首尾窗口必须有硬上限: {}", lines.len());
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.plain.contains("省略"))
                .count(),
            1
        );
        assert!(lines.iter().any(|line| line.plain.contains("item_0")));
        assert!(lines.iter().any(|line| line.plain.contains("item_100000")));
        assert!(
            lines
                .iter()
                .any(|line| line.plain.trim_start().starts_with("100042")),
            "尾部必须保留原文件真实行号"
        );
    }

    #[test]
    fn line_over_render_limit_is_utf8_safely_truncated() {
        let long = format!("{}尾", "界".repeat((4 * 1024 * 1024) / 3 + 10));
        let data = serde_json::json!({"old": long, "new": "短行", "start_line": 7});

        let (lines, snapshot) = crate::tui::render::performance::capture(|| {
            render_edit_diff(
                Some(&data),
                Some(r#"{"file_path":"src/lib.rs"}"#),
                "edited src/lib.rs",
                80,
            )
            .unwrap()
        });

        assert_eq!(snapshot.syntax_highlight_calls, 0);
        let removed = lines
            .iter()
            .find(|line| line.plain.contains("- "))
            .expect("删除行存在");
        assert!(removed.plain.len() < 4 * 1024 * 1024);
        assert!(removed.plain.contains("单行已截断"));
        assert!(std::str::from_utf8(removed.plain.as_bytes()).is_ok());
    }
}
