use super::*;

fn view(options: &[&str], cursor: usize, multi: bool) -> AskUserBlockView {
    AskUserBlockView {
        key: "ask".into(),
        question: "选哪个?".into(),
        options: options
            .iter()
            .map(|s| sdk::OptionItem::title_only(s.to_string()))
            .collect(),
        llm_option_count: options.len(),
        multi_select: multi,
        cursor,
        selected: vec![false; options.len()],
        chat_input_active: false,
        chat_input_text: String::new(),
        default: None,
        answer: None,
    }
}

#[test]
fn test_render_ask_user_highlights_cursor_option() {
    let block = render_ask_user(
        "ask",
        &view(&["A", "B"], 1, false),
        &RenderCtx { width: 80 },
    );
    // 找到选项 B 行（含 ❯ 标记表示高亮）
    let highlighted = block
        .lines
        .iter()
        .find(|line| line.plain.contains("2. B"))
        .expect("option B line");
    assert!(highlighted.plain.contains('❯'));
    assert_eq!(highlighted.spans[0].style.fg, Some(theme::WARNING));
    // 选项 A 不应高亮
    let other = block
        .lines
        .iter()
        .find(|line| line.plain.contains("1. A"))
        .expect("option A line");
    assert!(!other.plain.contains('❯'));
}

#[test]
fn test_render_ask_user_empty_options_shows_confirm_hint() {
    let mut v = view(&[], 0, false);
    v.options.clear();
    v.llm_option_count = 0;
    let block = render_ask_user("ask", &v, &RenderCtx { width: 80 });
    assert!(block
        .lines
        .iter()
        .any(|line| line.plain.contains("[Enter] 确认")));
    // 无选项时不渲染 ↑↓ 选择提示
    assert!(!block.lines.iter().any(|line| line.plain.contains("[↑↓]")));
}

#[test]
fn test_render_ask_user_empty_options_shows_default_line() {
    // 自由输入模式携带 default 时应渲染 `(default: ...)` 行（迁移回归）。
    let mut v = view(&[], 0, false);
    v.options.clear();
    v.llm_option_count = 0;
    v.default = Some("main".into());
    let block = render_ask_user("ask", &v, &RenderCtx { width: 80 });
    assert!(
        block
            .lines
            .iter()
            .any(|line| line.plain.contains("(default: main)")),
        "应渲染 default 提示行"
    );
}

#[test]
fn test_render_ask_user_empty_options_no_default_omits_line() {
    // 边界：无 default 时不渲染 `(default:` 行。
    let mut v = view(&[], 0, false);
    v.options.clear();
    v.llm_option_count = 0;
    let block = render_ask_user("ask", &v, &RenderCtx { width: 80 });
    assert!(!block
        .lines
        .iter()
        .any(|line| line.plain.contains("(default:")));
}

#[test]
fn test_render_ask_user_single_option_renders_marker() {
    let block = render_ask_user("ask", &view(&["Only"], 0, false), &RenderCtx { width: 80 });
    let line = block
        .lines
        .iter()
        .find(|line| line.plain.contains("1. Only"))
        .expect("only option");
    assert!(line.plain.contains('❯'));
}

#[test]
fn test_render_ask_user_multi_select_shows_checkbox() {
    let mut v = view(&["A", "B"], 0, true);
    v.selected = vec![false, true];
    let block = render_ask_user("ask", &v, &RenderCtx { width: 80 });
    let checked = block
        .lines
        .iter()
        .find(|line| line.plain.contains("2. B"))
        .expect("option B");
    assert!(checked.plain.contains("[✓]"));
}

#[test]
fn test_render_ask_user_chat_input_active_suppresses_option_highlight() {
    let mut v = view(&["A", "B"], 0, false);
    v.chat_input_active = true;
    let block = render_ask_user("ask", &v, &RenderCtx { width: 80 });
    // chat 子态下选项列表中无 ❯ 高亮
    let option_lines: Vec<_> = block
        .lines
        .iter()
        .filter(|line| line.plain.contains("1. ") || line.plain.contains("2. "))
        .collect();
    assert!(!option_lines.iter().any(|line| line.plain.contains('❯')));
    // Type something 输入框有 ❯
    assert!(block
        .lines
        .iter()
        .any(|line| line.plain.contains("Type something")));
}

#[test]
fn test_option_lines_with_description() {
    let item = sdk::OptionItem::new("Title", "Description line");
    let rendered = option_lines(0, &item, false, false);
    // title line + description line
    assert_eq!(rendered.len(), 2);
    assert!(rendered[0].0.contains("1. Title"));
    assert!(rendered[1].0.contains("Description line"));
    // description 有覆盖样式
    assert!(rendered[1].1.is_some());
}

#[test]
fn test_option_lines_title_only() {
    let item = sdk::OptionItem::title_only("Simple");
    let rendered = option_lines(0, &item, true, true);
    assert_eq!(rendered.len(), 1);
    assert!(rendered[0].0.contains("1. Simple"));
    // 无覆盖样式（由 active 控制）
    assert!(rendered[0].0.contains("[✓]"));
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
fn test_wrap_text_preserves_newlines() {
    let lines = wrap_text("line1\nline2", 80);
    assert_eq!(lines, vec!["line1", "line2"]);
}

#[test]
fn test_wrap_text_empty_paragraph() {
    let lines = wrap_text("before\n\nafter", 80);
    assert_eq!(lines, vec!["before", "", "after"]);
}

#[test]
fn test_wrap_text_zero_width_returns_raw_lines() {
    let lines = wrap_text("aaa bbb\nccc", 0);
    assert_eq!(lines, vec!["aaa bbb", "ccc"]);
}

#[test]
fn test_wrap_text_chinese_wraps_by_char() {
    // 每个中文字符约 2 列宽，20 列放约 10 个字
    let lines = wrap_text("这是一段很长的中文文本用来测试自动换行", 20);
    assert!(lines.len() >= 2, "中文长文本应被拆为多行，实际: {lines:?}");
    for line in &lines {
        assert!(
            line.width() <= 20,
            "每行不应超过 20 列: {line:?} ({} 列)",
            line.width()
        );
    }
}

#[test]
fn test_render_ask_user_answered_shows_answer_text() {
    let mut v = view(&["A", "B"], 0, false);
    v.answer = Some("都不喜欢".to_string());
    let block = render_ask_user("ask", &v, &RenderCtx { width: 80 });
    // 应包含回答文本
    assert!(
        block.lines.iter().any(|l| l.plain.contains("❯ 都不喜欢")),
        "应显示回答文本: {:?}",
        block.lines
    );
    // 不应显示选项列表或键盘提示
    assert!(
        !block.lines.iter().any(|l| l.plain.contains("1. A")),
        "已回答时不应显示选项"
    );
    assert!(
        !block.lines.iter().any(|l| l.plain.contains("[Enter]")),
        "已回答时不应显示键盘提示"
    );
}

#[test]
fn test_wrap_text_mixed_cjk_and_ascii() {
    let lines = wrap_text("hello 这是一段中文 world 更多中文内容", 20);
    assert!(lines.len() >= 2, "混合文本应换行，实际: {lines:?}");
}

#[test]
fn test_render_ask_user_wraps_long_question() {
    let mut v = view(&[], 0, false);
    v.question = "这是一段很长的提问内容用于测试自动换行功能是否正常工作".to_string();
    let block = render_ask_user("ask", &v, &RenderCtx { width: 60 });
    // 60 * 0.6 = 36，中文每字约 2 列宽，36 列放约 18 个字，所以应拆为多行
    let question_lines: Vec<_> = block
        .lines
        .iter()
        .skip(2) // 跳过 header + 空行
        .take_while(|l| !l.plain.contains('[') && !l.plain.is_empty())
        .collect();
    assert!(
        question_lines.len() >= 2,
        "长问题应被拆为多行，实际: {question_lines:?}"
    );
}
