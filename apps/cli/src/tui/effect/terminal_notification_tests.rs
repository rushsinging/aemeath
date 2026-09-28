use super::{
    format_osc777_notification, latest_user_prompt, turn_complete_notification,
    write_terminal_notification, TurnCompleteNotificationContext,
};
use crate::tui::model::output_timeline::OutputTimelineItem;

fn user_message(text: &str) -> OutputTimelineItem {
    OutputTimelineItem::UserMessage {
        id: "block-test".to_string(),
        text: text.to_string(),
    }
}

fn context() -> TurnCompleteNotificationContext<'static> {
    TurnCompleteNotificationContext {
        prompt: None,
        path_base: None,
        branch: None,
        duration_ms: None,
    }
}

#[test]
fn osc777_notification_formats_title_body_with_bel_terminator() {
    let sequence = format_osc777_notification("aemeath", "Turn complete");
    assert_eq!(sequence, "\x1b]777;notify;aemeath;Turn complete\x07");
}

#[test]
fn osc777_notification_strips_control_chars_from_title_and_body() {
    let sequence = format_osc777_notification("a\x07emeath", "done\x1b[31m\nnow");
    assert_eq!(sequence, "\x1b]777;notify;aemeath;done[31mnow\x07");
}

#[test]
fn osc777_notification_replaces_field_separator_in_title() {
    // 标题字段以 `;` 分隔，标题内的 `;` 必须替换，否则正文被吞进标题。
    let sequence = format_osc777_notification("a;b", "body");
    assert_eq!(sequence, "\x1b]777;notify;a:b;body\x07");
}

#[test]
fn write_terminal_notification_writes_exact_bytes_to_writer() {
    let mut written = Vec::new();
    write_terminal_notification(&mut written, "aemeath", "Turn complete")
        .expect("write to in-memory writer");
    assert_eq!(written, b"\x1b]777;notify;aemeath;Turn complete\x07");
}

/// 标题：项目名进 title，无项目时回退纯 "aemeath"。
#[test]
fn notification_title_appends_path_base_when_present() {
    let (title, _) = turn_complete_notification(&TurnCompleteNotificationContext {
        path_base: Some("~/repo/cli"),
        ..context()
    });
    assert_eq!(title, "aemeath · ~/repo/cli");
}

#[test]
fn notification_title_is_plain_without_path_base() {
    let (title, _) = turn_complete_notification(&context());
    assert_eq!(title, "aemeath");
}

/// 正文段落：prompt · branch · Turn complete in <耗时>，缺失段省略（无多余分隔符）。
#[test]
fn notification_body_joins_prompt_branch_duration() {
    let (_, body) = turn_complete_notification(&TurnCompleteNotificationContext {
        prompt: Some("重构通知逻辑"),
        branch: Some("feature/osc777"),
        duration_ms: Some(125_000),
        ..context()
    });
    assert_eq!(
        body,
        "重构通知逻辑 · feature/osc777 · Turn complete in 2m 5s"
    );
}

#[test]
fn notification_body_omits_missing_segments() {
    let (_, body) = turn_complete_notification(&TurnCompleteNotificationContext {
        branch: Some("main"),
        ..context()
    });
    assert_eq!(body, "main · Turn complete");

    let (_, body) = turn_complete_notification(&TurnCompleteNotificationContext {
        prompt: Some("hi"),
        duration_ms: Some(5_000),
        ..context()
    });
    assert_eq!(body, "hi · Turn complete in 5s");
}

#[test]
fn notification_body_without_any_context_is_plain_complete_notice() {
    let (_, body) = turn_complete_notification(&context());
    assert_eq!(body, "Turn complete");
}

/// prompt 摘要：取首行、超 60 字符截断加省略号。
#[test]
fn latest_user_prompt_takes_first_line_and_truncates_at_60_chars() {
    let first_line =
        latest_user_prompt(&[user_message("第一行说明\n第二行不该出现")]).expect("有用户消息");
    assert_eq!(first_line, "第一行说明");

    let long = latest_user_prompt(&[user_message(&"长".repeat(70))]).expect("有用户消息");
    assert_eq!(long.chars().count(), 61, "60 字符 + 省略号");
    assert!(long.ends_with('…'));
}

/// 取最后一条非 system-reminder 的用户消息；纯内部注入不进通知。
#[test]
fn latest_user_prompt_picks_last_real_user_message_and_skips_system_reminder() {
    let items = [
        user_message("最早的问题"),
        user_message("<system-reminder>Skill loaded</system-reminder>"),
        user_message("最新的问题"),
    ];
    assert_eq!(latest_user_prompt(&items).as_deref(), Some("最新的问题"));
}

#[test]
fn latest_user_prompt_is_none_without_user_message() {
    assert_eq!(latest_user_prompt(&[]), None);
}
