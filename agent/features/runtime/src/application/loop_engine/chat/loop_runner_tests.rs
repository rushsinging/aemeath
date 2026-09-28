include!("session_driver_support_tests.rs");
#[path = "resume_task_state_tests.rs"]
mod resume_task_state_tests;
include!("session_driver_retry_tests.rs");
include!("session_driver_stop_hook_tests.rs");
include!("session_driver_session_lifecycle_tests.rs");
include!("session_driver_input_adoption_tests.rs");
include!("session_driver_streaming_tools_tests.rs");

/// LLM 视图中的 user 输入带 `metadata.created_at` 时间前缀
/// `[YYYY-MM-DD HH:MM ±ZZZZ] `；断言 canonical 文本时先剥离。
/// 无前缀或非时间前缀的文本原样返回。
fn without_input_timestamp(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('[') else {
        return text;
    };
    let Some(end) = rest.find("] ") else {
        return text;
    };
    let stamp = &rest.as_bytes()[..end];
    let looks_like_timestamp = stamp.len() == 22
        && stamp[4] == b'-'
        && stamp[7] == b'-'
        && stamp[10] == b' '
        && stamp[13] == b':'
        && stamp[16] == b' '
        && (stamp[17] == b'+' || stamp[17] == b'-')
        && stamp.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 17) || byte.is_ascii_digit()
        });
    if looks_like_timestamp {
        &rest[end + 2..]
    } else {
        text
    }
}
