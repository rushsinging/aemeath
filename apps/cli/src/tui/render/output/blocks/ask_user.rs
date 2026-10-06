//! AskUserBatch 交互块渲染：批量问题 + 确认页。
//!
//! 三阶段渲染：
//! - **Answering**：显示进度 + 已答折叠摘要 + 当前激活问题选项列表
//! - **Confirming**：所有 Q→A 摘要列表 + 提交/取消操作
//! - **Confirmed**（终态）：简洁的 Q→A 列表

use crate::tui::render::output::primitives::wrap::{wrap_spans_with_prefix, WrapMode};
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::{
    AskUserBatchBlockView, AskUserCompletionView, AskUserPhaseView, AskUserSlotView,
};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use sdk::OptionItem;
use std::rc::Rc;
use unicode_width::UnicodeWidthStr;

/// 渲染单个选项的行。
///
/// `active` 控制 marker（❯/✓）；title 行使用 `title_style`；
/// description 按 `max_width` 自动换行（word-aware），续行缩进与 title 对齐，
/// description 固定 `TEXT_DIM` 样式。
fn option_lines(
    index: usize,
    option: &OptionItem,
    active: bool,
    title_style: Style,
    multi_select: bool,
    max_width: usize,
) -> Vec<RenderedLine> {
    let prefix = if multi_select {
        let check = if active { "✓" } else { " " };
        format!("  [{check}] {}. ", index + 1)
    } else {
        let marker = if active { "❯" } else { " " };
        format!("  {marker} {}. ", index + 1)
    };
    let prefix_width = prefix.width();
    let mut result = Vec::new();

    // title 行（单行，不 wrap）
    result.push(RenderedLine::new(vec![Span::styled(
        format!("{prefix}{}", option.title),
        title_style,
    )]));

    // description：按剩余可用宽度 word-wrap，续行缩进对齐 title 文本起始列
    if let Some(desc) = &option.description {
        let avail = max_width.saturating_sub(prefix_width);
        let continuation = Span::raw(" ".repeat(prefix_width));
        let desc_style = Style::default().fg(theme::TEXT_DIM);
        let wrapped = wrap_spans_with_prefix(
            vec![Span::styled(desc.clone(), desc_style)],
            avail,
            Some(continuation),
            WrapMode::Word,
        );
        result.extend(wrapped);
    }

    result
}

/// 截断文本到指定显示宽度（尾部加 `…`）。
fn truncate(text: &str, max_width: usize) -> String {
    if text.width() <= max_width {
        return text.to_string();
    }
    let mut result = String::new();
    let mut width = 0;
    for ch in text.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + cw + 1 > max_width {
            break;
        }
        result.push(ch);
        width += cw;
    }
    result.push('…');
    result
}

/// 构造 Type something 输入行的 spans：按 `chat_input_cursor`（byte offset，
/// 先钳制到合法 char 边界）拆成 before / 光标字符 / after 三段，光标字符
/// 用 ACCENT 背景块状高亮；光标在文本末尾时高亮一个空格。
fn chat_input_spans(input_text: &str, chat_input_cursor: usize) -> Vec<Span<'static>> {
    let raw_cursor = chat_input_cursor.min(input_text.len());
    let cursor = input_text.floor_char_boundary(raw_cursor);
    let before = input_text.get(..cursor).unwrap_or("");
    let after = input_text.get(cursor..).unwrap_or("");
    let cursor_style = Style::default().bg(theme::ACCENT).fg(theme::BASE);
    vec![
        Span::raw(before.to_string()),
        Span::styled(
            after
                .chars()
                .next()
                .map(|c| c.to_string())
                .unwrap_or_else(|| " ".to_string()),
            cursor_style,
        ),
        Span::raw(after.chars().skip(1).collect::<String>()),
    ]
}

/// 渲染 Q→A 摘要行（用于确认页和折叠摘要）。
fn qa_summary_lines(
    index: usize,
    slot: &AskUserSlotView,
    active: bool,
    max_width: usize,
) -> Vec<RenderedLine> {
    let answer = slot.answer.as_deref().unwrap_or("（未回答）");
    let q_line = if active {
        format!(
            "  ❯ Q{}. {}",
            index + 1,
            truncate(&slot.question, max_width.saturating_sub(8))
        )
    } else {
        format!(
            "Q{}. {}",
            index + 1,
            truncate(&slot.question, max_width.saturating_sub(4))
        )
    };
    let a_line = if active {
        format!("      ❯ {}", truncate(answer, max_width.saturating_sub(8)))
    } else {
        format!("  ❯ {}", truncate(answer, max_width.saturating_sub(4)))
    };

    let q_style = if active {
        Style::default()
            .fg(theme::WARNING)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    let a_style = if active {
        Style::default().fg(theme::SUCCESS)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };

    vec![
        RenderedLine::new(vec![Span::styled(q_line, q_style)]),
        RenderedLine::new(vec![Span::styled(a_line, a_style)]),
    ]
}

pub fn render_ask_user_batch(
    block_id: &str,
    view: &AskUserBatchBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    let header_style = Style::default()
        .fg(theme::WARNING)
        .add_modifier(Modifier::BOLD);
    let hint_style = Style::default().fg(theme::TEXT_DIM);
    let normal_style = Style::default().fg(theme::TEXT);

    let question_max_width = (ctx.text_width as usize * 6 / 10).clamp(40, 80);

    match view.completion {
        AskUserCompletionView::Answered => {
            return render_terminal(block_id, view, "━━ 已回答 ━━", question_max_width, true);
        }
        AskUserCompletionView::Cancelled => {
            return render_terminal(block_id, view, "━━ 已取消 ━━", question_max_width, false);
        }
        AskUserCompletionView::ReplyPending => {
            return render_pending(block_id, view, "━━ 正在提交回答… ━━", question_max_width);
        }
        AskUserCompletionView::CancelPending => {
            return render_pending(block_id, view, "━━ 正在取消… ━━", question_max_width);
        }
        AskUserCompletionView::Active => {}
    }

    match view.phase {
        AskUserPhaseView::Answering => render_answering(
            block_id,
            view,
            ctx,
            header_style,
            hint_style,
            normal_style,
            question_max_width,
        ),
        AskUserPhaseView::Confirming => {
            render_confirming(block_id, view, header_style, hint_style, question_max_width)
        }
    }
}

/// Answering 阶段渲染。
fn render_answering(
    block_id: &str,
    view: &AskUserBatchBlockView,
    ctx: &RenderCtx,
    header_style: Style,
    hint_style: Style,
    normal_style: Style,
    question_max_width: usize,
) -> RenderedBlock {
    let mut lines = Vec::new();
    let total = view.slots.len();
    let current = view.active_index + 1;

    // Header 带进度
    let header_text = if total > 1 {
        format!("━━ 需要你的回答 ({current}/{total}) ━━")
    } else {
        "━━ 需要你的回答 ━━".to_string()
    };
    lines.push(RenderedLine::new(vec![Span::styled(
        header_text,
        header_style,
    )]));

    // 已答 slot 折叠摘要
    for (i, slot) in view.slots.iter().enumerate() {
        if i == view.active_index {
            continue;
        }
        if let Some(answer) = &slot.answer {
            lines.push(RenderedLine::new(vec![Span::styled(
                format!(
                    "  ✓ Q{}. {} → {}",
                    i + 1,
                    truncate(&slot.question, 30),
                    truncate(answer, 30)
                ),
                hint_style,
            )]));
        }
    }

    lines.push(RenderedLine::new(vec![Span::raw("")]));

    // 当前激活问题（按段落 wrap，空段保留空行）
    let active_slot = &view.slots[view.active_index];
    for paragraph in active_slot.question.split('\n') {
        if paragraph.is_empty() {
            lines.push(RenderedLine::empty());
            continue;
        }
        lines.extend(wrap_spans_with_prefix(
            vec![Span::styled(paragraph.to_string(), header_style)],
            question_max_width,
            None,
            WrapMode::Word,
        ));
    }

    let multi = active_slot.multi_select;

    // 自由输入模式（无选项）
    if active_slot.options.is_empty() {
        if let Some(d) = &active_slot.default {
            lines.push(RenderedLine::new(vec![Span::styled(
                format!("  (default: {d})"),
                hint_style,
            )]));
        }
        lines.push(RenderedLine::new(vec![Span::raw("")]));
        // Type something 输入框（带光标）
        let mut spans = vec![Span::styled("  ❯ Type something: ", header_style)];
        spans.extend(chat_input_spans(
            &view.chat_input_text,
            view.chat_input_cursor,
        ));
        lines.push(RenderedLine::new(spans));
        lines.push(RenderedLine::new(vec![Span::raw("")]));
        lines.push(RenderedLine::new(vec![Span::styled(
            "  [Enter] 确认  [Esc] 取消  [←→] 移动光标  [Ctrl+W] 删词".to_string(),
            hint_style,
        )]));
        return RenderedBlock {
            block_id: block_id.to_string(),
            lines: Rc::new(lines),
        };
    }

    // 有选项模式
    let hint = if multi {
        "  [↑↓] 移动  [Space] 选中/取消  [Enter] 确认  [Esc] 取消"
    } else {
        "  [↑↓] 选择  [Enter] 确认  [Esc] 取消"
    };
    lines.push(RenderedLine::new(vec![Span::styled(
        hint.to_string(),
        hint_style,
    )]));
    lines.push(RenderedLine::new(vec![Span::raw("")]));

    for (i, option) in active_slot.options.iter().enumerate() {
        let is_cursor = !view.chat_input_active && i == view.cursor;
        let is_checked = multi && view.selected.get(i).copied().unwrap_or(false);
        let active = is_cursor || is_checked;
        let title_style = if active { header_style } else { normal_style };
        lines.extend(option_lines(
            i,
            option,
            active,
            title_style,
            multi,
            ctx.text_width as usize,
        ));
    }

    // Type something 子态（LLM 选项中的最后一项被选中时激活）
    if view.chat_input_active {
        lines.push(RenderedLine::new(vec![Span::raw("")]));
        let mut spans = vec![Span::styled("  ❯ Type something: ", header_style)];
        spans.extend(chat_input_spans(
            &view.chat_input_text,
            view.chat_input_cursor,
        ));
        lines.push(RenderedLine::new(spans));
    }

    lines.push(RenderedLine::new(vec![Span::raw("")]));
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

/// Confirming 阶段渲染。
fn render_confirming(
    block_id: &str,
    view: &AskUserBatchBlockView,
    header_style: Style,
    hint_style: Style,
    question_max_width: usize,
) -> RenderedBlock {
    let mut lines = Vec::new();
    lines.push(RenderedLine::new(vec![Span::styled(
        "━━ 确认回答 ━━".to_string(),
        header_style,
    )]));
    lines.push(RenderedLine::new(vec![Span::raw("")]));

    // Q→A 列表
    for (i, slot) in view.slots.iter().enumerate() {
        let is_cursor = i == view.confirm_cursor;
        for line in qa_summary_lines(i, slot, is_cursor, question_max_width) {
            lines.push(line);
        }
    }

    lines.push(RenderedLine::new(vec![Span::raw("")]));

    // 提交按钮（confirm_cursor == N）
    let submit_active = view.confirm_cursor == view.slots.len();
    let submit_marker = if submit_active { "❯" } else { " " };
    let submit_style = if submit_active {
        Style::default()
            .fg(theme::SUCCESS)
            .add_modifier(Modifier::BOLD)
    } else {
        hint_style
    };
    lines.push(RenderedLine::new(vec![Span::styled(
        format!("  {submit_marker} ✓ 全部确认提交"),
        submit_style,
    )]));

    // 取消按钮（confirm_cursor == N+1）
    let cancel_active = view.confirm_cursor == view.slots.len() + 1;
    let cancel_marker = if cancel_active { "❯" } else { " " };
    let cancel_style = if cancel_active {
        Style::default()
            .fg(theme::WARNING)
            .add_modifier(Modifier::BOLD)
    } else {
        hint_style
    };
    lines.push(RenderedLine::new(vec![Span::styled(
        format!("  {cancel_marker} ✗ 取消"),
        cancel_style,
    )]));

    lines.push(RenderedLine::new(vec![Span::raw("")]));
    lines.push(RenderedLine::new(vec![Span::styled(
        "  [↑↓] 导航  [Enter] 选择/确认/重新作答  [Esc] 取消".to_string(),
        hint_style,
    )]));

    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

fn render_pending(
    block_id: &str,
    view: &AskUserBatchBlockView,
    title: &str,
    question_max_width: usize,
) -> RenderedBlock {
    render_terminal(block_id, view, title, question_max_width, true)
}

fn render_terminal(
    block_id: &str,
    view: &AskUserBatchBlockView,
    title: &str,
    question_max_width: usize,
    show_answers: bool,
) -> RenderedBlock {
    let dim_style = Style::default().fg(theme::TEXT_DIM);
    let mut lines = vec![RenderedLine::new(vec![Span::styled(
        title.to_string(),
        dim_style,
    )])];

    for (index, slot) in view.slots.iter().enumerate() {
        lines.push(RenderedLine::new(vec![Span::raw("")]));
        if show_answers {
            lines.extend(qa_summary_lines(index, slot, false, question_max_width));
        } else {
            lines.extend(wrap_spans_with_prefix(
                vec![Span::styled(
                    format!("Q{}. {}", index + 1, slot.question),
                    dim_style,
                )],
                question_max_width,
                None,
                WrapMode::Word,
            ));
        }
    }

    lines.push(RenderedLine::new(vec![Span::raw("")]));
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

#[cfg(test)]
#[path = "ask_user_tests.rs"]
mod tests;
