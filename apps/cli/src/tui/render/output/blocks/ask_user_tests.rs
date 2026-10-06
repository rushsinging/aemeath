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
        chat_input_cursor: 0,
        confirm_cursor: 0,
        completion: AskUserCompletionView::Active,
    }
}

#[test]
fn test_answering_shows_progress_header() {
    let view = batch_view(
        vec![make_slot("问题1", &["A"]), make_slot("问题2", &["B"])],
        0,
        AskUserPhaseView::Answering,
    );
    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));
    assert!(block.lines.iter().any(|l| l.plain.contains("(1/2)")));
}

#[test]
fn test_answering_shows_current_question() {
    let view = batch_view(
        vec![make_slot("选哪个?", &["A", "B"])],
        0,
        AskUserPhaseView::Answering,
    );
    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));
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
    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));
    assert!(block.lines.iter().any(|l| l.plain.contains("✓ Q1.")));
}

#[test]
fn test_confirming_shows_qa_list_and_actions() {
    let mut s1 = make_slot("问题1", &["A"]);
    s1.answer = Some("A".to_string());
    let view = batch_view(vec![s1], 0, AskUserPhaseView::Confirming);
    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));
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
    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));
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
    view.completion = AskUserCompletionView::Answered;
    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));
    assert!(block.lines.iter().any(|l| l.plain.contains("已回答")));
    assert!(!block.lines.iter().any(|l| l.plain.contains("[↑↓]")));
}

#[test]
fn test_cancelled_shows_question_without_answer_or_controls() {
    let mut slot = make_slot("问题1", &["A"]);
    slot.answer = Some("A".to_string());
    let mut view = batch_view(vec![slot], 0, AskUserPhaseView::Answering);
    view.completion = AskUserCompletionView::Cancelled;

    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));

    assert!(block.lines.iter().any(|line| line.plain.contains("已取消")));
    assert!(block.lines.iter().any(|line| line.plain.contains("问题1")));
    assert!(!block.lines.iter().any(|line| line.plain.contains("→ A")));
    assert!(!block.lines.iter().any(|line| line.plain.contains("[↑↓]")));
}

#[test]
fn test_reply_pending_shows_answers_without_claiming_acceptance() {
    let mut slot = make_slot("问题1", &["A"]);
    slot.answer = Some("A".to_string());
    let mut view = batch_view(vec![slot], 0, AskUserPhaseView::Answering);
    view.completion = AskUserCompletionView::ReplyPending;

    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));

    assert!(block
        .lines
        .iter()
        .any(|line| line.plain.contains("正在提交回答")));
    assert!(block.lines.iter().any(|line| line.plain.contains("A")));
    assert!(!block.lines.iter().any(|line| line.plain.contains("已回答")));
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
    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));
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
fn test_chat_input_with_options_renders_block_cursor_at_cursor_offset() {
    // 有选项 + Type something 子态：光标 span 必须覆盖 chat_input_cursor
    // 处的字符，而非固定画在文本末尾。
    let mut view = batch_view(
        vec![make_slot("选哪个?", &["A"])],
        0,
        AskUserPhaseView::Answering,
    );
    view.chat_input_active = true;
    view.chat_input_text = "hello".to_string();
    view.chat_input_cursor = 2; // 光标应覆盖 'l'（第 3 个字符）
    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(80));
    let type_line = block
        .lines
        .iter()
        .find(|l| l.plain.contains("Type something:"))
        .expect("type something input line");
    let cursor_span = type_line
        .spans
        .iter()
        .find(|s| s.style.bg.is_some())
        .expect("块状光标 span");
    assert_eq!(cursor_span.content.as_ref(), "l");
}

#[test]
fn test_answering_wraps_long_option_description() {
    // issue #403：长 description 应按可用宽度自动换行，而非整段溢出
    let long_desc =
        "这是一段很长的选项描述文本用来测试在窄终端宽度下是否会被自动换行而不溢出显示边界";
    let opt = sdk::OptionItem::new("选项A", long_desc);
    let mut options = vec![opt];
    options.push(sdk::OptionItem::title_only("Type something...".to_string()));
    let slot = AskUserSlotView {
        id: "q-1".into(),
        question: "选哪个?".into(),
        options,
        llm_option_count: 1,
        multi_select: false,
        default: None,
        answer: None,
    };
    let view = batch_view(vec![slot], 0, AskUserPhaseView::Answering);
    let block = render_ask_user_batch("ask", &view, &RenderCtx::for_width(40));

    // description 内容必须可见（未被丢弃）
    assert!(
        block.lines.iter().any(|l| l.plain.contains("自动换行")),
        "description 应可见: {:?}",
        block.lines.iter().map(|l| &l.plain).collect::<Vec<_>>()
    );
    // 任何行都不应超过可用宽度（核心：长 description 应换行而非溢出）
    for l in block.lines.iter() {
        assert!(
            l.plain.width() <= 40,
            "行宽超过 40: {:?} ({} 列)",
            l.plain,
            l.plain.width()
        );
    }
}

#[test]
fn test_option_lines_wraps_description_with_continuation_indent() {
    // 续行应与 title 文本起始列对齐（prefix 宽度）
    let opt = sdk::OptionItem::new("标题", "aaa bbb ccc ddd eee fff");
    let lines = option_lines(0, &opt, true, Style::default(), false, 12);
    // 第一行是 title
    assert!(lines[0].plain.contains("1. 标题"));
    // 后续行是 description 续行，应以空格缩进对齐
    for l in lines.iter().skip(1) {
        assert!(
            l.plain.starts_with(' '),
            "description 续行应缩进: {:?}",
            l.plain
        );
        assert!(l.plain.width() <= 12, "续行不超宽: {:?}", l.plain);
    }
}
