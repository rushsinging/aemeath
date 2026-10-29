use super::*;

#[test]
fn parse_json_items_strips_markdown_fences() {
    let raw = "```json\n[{\"question\":\"目标是什么\",\"gold\":\"修复登录\"}]\n```";
    let items = parse_json_items(raw).expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].question, "目标是什么");
    assert_eq!(items[0].gold.as_deref(), Some("修复登录"));
}

#[test]
fn parse_json_items_rejects_prose() {
    assert!(parse_json_items("这是解释文字").is_none());
    assert!(parse_json_items("").is_none());
}

#[test]
fn truncate_by_tokens_respects_cjk_weight() {
    let cjk = "中".repeat(100);
    let truncated = truncate_by_tokens(&cjk, 6);
    // 每个中文字符 0.6 token → 6 token 预算约 10 字。
    assert_eq!(truncated.chars().count(), 10);

    let ascii = "a".repeat(100);
    let truncated = truncate_by_tokens(&ascii, 3);
    assert_eq!(truncated.chars().count(), 10);
}

#[test]
fn truncate_by_tokens_keeps_short_text_intact() {
    assert_eq!(truncate_by_tokens("短文本", 60_000), "短文本");
}

#[test]
fn question_count_floats_with_early_size() {
    // 每 40K token 一题，clamp [10, 30]
    assert_eq!(question_count_for(100_000), 10);
    assert_eq!(question_count_for(814_000), 20);
    assert_eq!(question_count_for(2_000_000), 30);
}
