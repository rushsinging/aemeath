use crate::domain::user_input_timestamp::{
    render_user_input_timestamp_prefix, strip_user_input_timestamp_prefix,
};

fn fixed_offset(source: &str) -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339(source).expect("探针时刻必须是合法 RFC3339")
}

#[test]
fn render_emits_minute_precision_offset_prefix() {
    let created_at = fixed_offset("2026-10-09T15:29:00+08:00");

    assert_eq!(
        render_user_input_timestamp_prefix(&created_at),
        "[2026-10-09 15:29 +0800]: "
    );
}

#[test]
fn strip_removes_rendered_prefix() {
    assert_eq!(
        strip_user_input_timestamp_prefix("[2026-10-09 15:29 +0800]: 开始 replan 设计"),
        "开始 replan 设计"
    );
}

#[test]
fn strip_removes_negative_offset_prefix() {
    assert_eq!(
        strip_user_input_timestamp_prefix("[2026-10-09 02:29 -0500]: move on"),
        "move on"
    );
}

#[test]
fn render_then_strip_restores_original_text() {
    let created_at = fixed_offset("2026-10-09T15:29:00+08:00");
    let original = "把当前决策落盘吧";
    let rendered = format!(
        "{}{original}",
        render_user_input_timestamp_prefix(&created_at)
    );

    assert_eq!(strip_user_input_timestamp_prefix(&rendered), original);
}

#[test]
fn strip_is_idempotent_on_plain_text() {
    let plain = "普通文本";
    let once = strip_user_input_timestamp_prefix(plain);

    assert_eq!(once, plain);
    assert_eq!(strip_user_input_timestamp_prefix(once), plain);
}

#[test]
fn strip_keeps_bracket_text_that_is_not_timestamp() {
    for source in [
        "[note]: 不是时间戳",
        "[2026-10-09 15:29]: 缺时区偏移",
        "[2026-10-09 15:29 +0800] 缺冒号空格",
        "[2026/10/09 15:29 +0800]: 非法日期分隔",
        "[2026-10-09 15:29 +080]: 非法偏移宽度",
        "2026-10-09 15:29 +0800: 缺方括号",
    ] {
        assert_eq!(
            strip_user_input_timestamp_prefix(source),
            source,
            "不应误伤：{source}"
        );
    }
}

#[test]
fn strip_only_touches_leading_prefix() {
    let source = "[2026-10-09 15:29 +0800]: 第一行\n[2026-10-09 16:00 +0800]: 第二行";

    assert_eq!(
        strip_user_input_timestamp_prefix(source),
        "第一行\n[2026-10-09 16:00 +0800]: 第二行"
    );
}

#[test]
fn strip_yields_empty_body_when_only_prefix_present() {
    assert_eq!(
        strip_user_input_timestamp_prefix("[2026-10-09 15:29 +0800]: "),
        ""
    );
}
