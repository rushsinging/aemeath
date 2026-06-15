//! AskUserBatch 交互块渲染：批量问题 + 确认页。
//!
//! 三阶段渲染：
//! - **Answering**：显示进度 + 已答折叠摘要 + 当前激活问题选项列表
//! - **Confirming**：所有 Q→A 摘要列表 + 提交/取消操作
//! - **Confirmed**（终态）：简洁的 Q→A 列表

use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::{AskUserBatchBlockView, AskUserPhaseView, AskUserSlotView};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use sdk::OptionItem;
use unicode_width::UnicodeWidthStr;

/// 将文本按指定显示列宽自动换行，返回多行字符串。
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return text.lines().map(|l| l.to_string()).collect();
    }
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut current_line = String::new();
        let mut current_width = 0;
        for word in paragraph.split_whitespace() {
            let word_width = word.width();
            let space = if current_line.is_empty() { 0 } else { 1 };
            if current_line.is_empty() {
                for chunk in split_into_chunks(word, max_width) {
                    if current_line.is_empty() {
                        current_line = chunk;
                        current_width = current_line.width();
                    } else {
                        result.push(current_line);
                        current_line = chunk;
                        current_width = current_line.width();
                    }
                }
            } else if current_width + space + word_width <= max_width {
                current_line.push(' ');
                current_line.push_str(word);
                current_width += space + word_width;
            } else {
                result.push(current_line);
                current_line = String::new();
                current_width = 0;
                for chunk in split_into_chunks(word, max_width) {
                    if current_line.is_empty() {
                        current_line = chunk;
                        current_width = current_line.width();
                    } else {
                        result.push(current_line);
                        current_line = chunk;
                        current_width = current_line.width();
                    }
                }
            }
        }
        if !current_line.is_empty() {
            result.push(current_line);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

fn split_into_chunks(s: &str, max_width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_w = 0;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_w + cw > max_width && !current.is_empty() {
            chunks.push(current);
            current = String::new();
            current_w = 0;
        }
        current.push(ch);
        current_w += cw;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// 渲染单个选项的行。
fn option_lines(
    index: usize,
    option: &OptionItem,
    active: bool,
    multi_select: bool,
) -> Vec<(String, Option<Style>)> {
    let prefix = if multi_select {
        let check = if active { "✓" } else { " " };
        format!("  [{check}] {}. ", index + 1)
    } else {
        let marker = if active { "❯" } else { " " };
        format!("  {marker} {}. ", index + 1)
    };
    let continuation = " ".repeat(prefix.chars().count());
    let mut lines = Vec::new();

    let title_line = format!("{prefix}{}", option.title);
    lines.push((title_line, None));

    if let Some(desc) = &option.description {
        for line in desc.lines() {
            lines.push((
                format!("{continuation}{line}"),
                Some(Style::default().fg(theme::TEXT_DIM)),
            ));
        }
    }

    if lines.is_empty() {
        lines.push((prefix, None));
    }
    lines
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

/// 渲染 Q→A 摘要行（用于确认页和折叠摘要）。
fn qa_summary_lines(
    index: usize,
    slot: &AskUserSlotView,
    active: bool,
    max_width: usize,
) -> Vec<RenderedLine> {
    let marker = if active { "❯" } else { " " };
    let answer = slot.answer.as_deref().unwrap_or("（未回答）");
    let q_line = format!(
        "  {marker} Q{}. {}",
        index + 1,
        truncate(&slot.question, max_width.saturating_sub(8))
    );
    let a_line = format!("      ❯ {}", truncate(answer, max_width.saturating_sub(8)));

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

    let question_max_width = (ctx.width as usize * 6 / 10).clamp(40, 80);

    // Confirmed 终态
    if view.confirmed {
        return render_confirmed(block_id, view, header_style, question_max_width);
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
    _ctx: &RenderCtx,
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

    // 当前激活问题
    let active_slot = &view.slots[view.active_index];
    for line in wrap_text(&active_slot.question, question_max_width) {
        lines.push(RenderedLine::new(vec![Span::styled(line, header_style)]));
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
        // Type something 输入框
        let input_text = &view.chat_input_text;
        let prompt = format!("  ❯ Type something: {input_text}");
        lines.push(RenderedLine::new(vec![
            Span::styled(prompt.clone(), header_style),
            Span::styled(" ", Style::default().bg(theme::ACCENT)),
        ]));
        lines.push(RenderedLine::new(vec![Span::raw("")]));
        lines.push(RenderedLine::new(vec![Span::styled(
            "  [Enter] 确认  [Esc] 取消".to_string(),
            hint_style,
        )]));
        return RenderedBlock {
            block_id: block_id.to_string(),
            lines,
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
        for (line_idx, (content, override_style)) in option_lines(i, option, active, multi)
            .into_iter()
            .enumerate()
        {
            let style = override_style.unwrap_or(if active && line_idx == 0 {
                header_style
            } else {
                normal_style
            });
            lines.push(RenderedLine::new(vec![Span::styled(content, style)]));
        }
    }

    // Type something 子态（LLM 选项中的最后一项被选中时激活）
    if view.chat_input_active {
        lines.push(RenderedLine::new(vec![Span::raw("")]));
        let input_text = &view.chat_input_text;
        let prompt = format!("  ❯ Type something: {input_text}");
        lines.push(RenderedLine::new(vec![
            Span::styled(prompt, header_style),
            Span::styled(" ", Style::default().bg(theme::ACCENT)),
        ]));
    }

    lines.push(RenderedLine::new(vec![Span::raw("")]));
    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
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
        lines,
    }
}

/// Confirmed 终态渲染。
fn render_confirmed(
    block_id: &str,
    view: &AskUserBatchBlockView,
    header_style: Style,
    question_max_width: usize,
) -> RenderedBlock {
    let dim_style = Style::default().fg(theme::TEXT_DIM);
    let mut lines = Vec::new();
    lines.push(RenderedLine::new(vec![Span::styled(
        "━━ 已回答 ━━".to_string(),
        dim_style,
    )]));

    for (i, slot) in view.slots.iter().enumerate() {
        lines.push(RenderedLine::new(vec![Span::raw("")]));
        for line in qa_summary_lines(i, slot, false, question_max_width) {
            lines.push(line);
        }
    }

    lines.push(RenderedLine::new(vec![Span::raw("")]));
    let _ = header_style;
    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_slot(question: &str, options: &[&str]) -> AskUserSlotView {
        let llm_count = options.len();
        let mut all = options
            .iter()
            .map(|s| sdk::OptionItem::title_only(s.to_string()))
            .collect::<Vec<_>>();
        if !all.is_empty() {
            all.push(sdk::OptionItem::title_only("Type something...".to_string()));
        }
        AskUserSlotView {
            id: format!("q-{}", question.len()),
            question: question.to_string(),
            options: all,
            llm_option_count: llm_count,
            multi_select: false,
            default: None,
            answer: None,
        }
    }

    fn batch_view(
        slots: Vec<AskUserSlotView>,
        active_index: usize,
        phase: AskUserPhaseView,
    ) -> AskUserBatchBlockView {
        let cursor = 0;
        let options_count = slots
            .get(active_index)
            .map(|s| s.options.len())
            .unwrap_or(0);
        AskUserBatchBlockView {
            key: "ask".into(),
            slots,
            active_index,
            phase,
            cursor,
            selected: vec![false; options_count],
            chat_input_active: false,
            chat_input_text: String::new(),
            confirm_cursor: 0,
            confirmed: false,
        }
    }

    #[test]
    fn test_answering_shows_progress_header() {
        let view = batch_view(
            vec![make_slot("问题1", &["A"]), make_slot("问题2", &["B"])],
            0,
            AskUserPhaseView::Answering,
        );
        let block = render_ask_user_batch("ask", &view, &RenderCtx { width: 80 });
        assert!(block.lines.iter().any(|l| l.plain.contains("(1/2)")));
    }

    #[test]
    fn test_answering_shows_current_question() {
        let view = batch_view(
            vec![make_slot("选哪个?", &["A", "B"])],
            0,
            AskUserPhaseView::Answering,
        );
        let block = render_ask_user_batch("ask", &view, &RenderCtx { width: 80 });
        assert!(block.lines.iter().any(|l| l.plain.contains("选哪个?")));
        assert!(block.lines.iter().any(|l| l.plain.contains("1. A")));
    }

    #[test]
    fn test_answering_shows_answered_summary() {
        let mut s1 = make_slot("问题1", &["A"]);
        s1.answer = Some("A".to_string());
        let view = batch_view(
            vec![s1, make_slot("问题2", &["B"])],
            1,
            AskUserPhaseView::Answering,
        );
        let block = render_ask_user_batch("ask", &view, &RenderCtx { width: 80 });
        assert!(block.lines.iter().any(|l| l.plain.contains("✓ Q1.")));
    }

    #[test]
    fn test_confirming_shows_qa_list_and_actions() {
        let mut s1 = make_slot("问题1", &["A"]);
        s1.answer = Some("A".to_string());
        let view = batch_view(vec![s1], 0, AskUserPhaseView::Confirming);
        let block = render_ask_user_batch("ask", &view, &RenderCtx { width: 80 });
        assert!(block.lines.iter().any(|l| l.plain.contains("确认回答")));
        assert!(block.lines.iter().any(|l| l.plain.contains("全部确认提交")));
        assert!(block.lines.iter().any(|l| l.plain.contains("取消")));
    }

    #[test]
    fn test_confirming_submit_highlighted_at_default_cursor() {
        let mut s1 = make_slot("问题1", &["A"]);
        s1.answer = Some("A".to_string());
        let mut view = batch_view(vec![s1], 0, AskUserPhaseView::Confirming);
        view.confirm_cursor = 1; // N=1 → 提交
        let block = render_ask_user_batch("ask", &view, &RenderCtx { width: 80 });
        let submit_line = block
            .lines
            .iter()
            .find(|l| l.plain.contains("全部确认提交"))
            .expect("submit line");
        assert!(submit_line.plain.contains('❯'));
    }

    #[test]
    fn test_confirmed_shows_simple_list() {
        let mut s1 = make_slot("问题1", &["A"]);
        s1.answer = Some("A".to_string());
        let mut view = batch_view(vec![s1], 0, AskUserPhaseView::Confirming);
        view.confirmed = true;
        let block = render_ask_user_batch("ask", &view, &RenderCtx { width: 80 });
        assert!(block.lines.iter().any(|l| l.plain.contains("已回答")));
        assert!(!block.lines.iter().any(|l| l.plain.contains("[↑↓]")));
    }

    #[test]
    fn test_chat_input_uses_block_cursor() {
        let mut view = batch_view(
            vec![make_slot("选哪个?", &["A"])],
            0,
            AskUserPhaseView::Answering,
        );
        view.chat_input_active = true;
        view.chat_input_text = "hello".to_string();
        let block = render_ask_user_batch("ask", &view, &RenderCtx { width: 80 });
        // Type something 行应包含块状光标（bg(ACCENT) 样式的 span）
        let type_line = block
            .lines
            .iter()
            .find(|l| l.plain.contains("Type something:"))
            .expect("type something input line");
        assert!(type_line.spans.iter().any(|s| s.style.bg.is_some()));
        // 不应有旧的 ▏ 竖线光标
        assert!(!type_line.plain.contains('▏'));
    }

    #[test]
    fn test_wrap_text_short_text_no_wrap() {
        let lines = wrap_text("hello world", 80);
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn test_wrap_text_long_text_wraps() {
        let lines = wrap_text("aaa bbb ccc ddd eee fff", 11);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "aaa bbb ccc");
        assert_eq!(lines[1], "ddd eee fff");
    }

    #[test]
    fn test_wrap_text_chinese_wraps_by_char() {
        let lines = wrap_text("这是一段很长的中文文本用来测试自动换行", 20);
        assert!(lines.len() >= 2);
        for line in &lines {
            assert!(line.width() <= 20);
        }
    }
}
