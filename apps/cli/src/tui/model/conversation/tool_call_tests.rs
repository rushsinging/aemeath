use super::*;
use crate::tui::model::conversation::ids::{ChatId, ChatRunId, ToolCallId, ToolStreamKey};

fn stream_key() -> ToolStreamKey {
    ToolStreamKey::new(ChatId::new("chat-1"), ChatRunId::new("turn-1"), "Read", 0)
}

use crate::tui::model::conversation::tool_result_payload::ToolResultPayload;

fn pending_call() -> ToolCall {
    ToolCall::pending(ToolCallId::new("tool-1"), stream_key())
}

fn bound_call() -> ToolCall {
    let mut call = ToolCall::pending(ToolCallId::new("tool-1"), stream_key());
    call.update(None, ToolCallStatus::Running);
    call
}

#[test]
fn test_tool_call_completes_success() {
    let mut call = bound_call();
    let payload = ToolResultPayload::new(
        "ok".to_string(),
        serde_json::json!({ "text": "ok" }),
        false,
        0,
    );
    call.complete(payload.clone());
    assert_eq!(call.status, ToolCallStatus::Success);
    assert_eq!(call.result.as_ref().map(|p| p.output.as_str()), Some("ok"));
    assert_eq!(
        call.result.as_ref().map(|p| &p.content),
        Some(&serde_json::json!({ "text": "ok" }))
    );
    assert_eq!(call.result, Some(payload));
}

#[test]
fn test_tool_call_completes_error() {
    let mut call = bound_call();
    call.complete(ToolResultPayload::new(
        "failed".to_string(),
        serde_json::json!({ "text": "failed" }),
        true,
        0,
    ));
    assert_eq!(call.status, ToolCallStatus::Error);
    assert_eq!(
        call.result.as_ref().map(|p| p.output.as_str()),
        Some("failed")
    );
    assert!(call.result.as_ref().is_some_and(|p| p.is_error));
}

#[test]
fn test_update_preserves_args_preview_when_arguments_none() {
    let mut call = pending_call();
    call.update(Some(r#"{"taskId":"42"}"#.into()), ToolCallStatus::Running);
    assert_eq!(call.args_preview, r#"{"taskId":"42"}"#);
    call.update(None, ToolCallStatus::Running);
    assert_eq!(
        call.args_preview, r#"{"taskId":"42"}"#,
        "None 不应覆盖已有 args_preview"
    );
}

#[test]
fn test_update_preserves_args_preview_when_arguments_empty_string() {
    let mut call = pending_call();
    call.update(Some(r#"{"taskId":"42"}"#.into()), ToolCallStatus::Running);
    assert_eq!(call.args_preview, r#"{"taskId":"42"}"#);
    call.update(Some(String::new()), ToolCallStatus::Running);
    assert_eq!(
        call.args_preview, r#"{"taskId":"42"}"#,
        "空字符串不应覆盖已有 args_preview"
    );
}

// ── issue #839：update_args 同样需要空值防护 ──

#[test]
fn test_update_args_empty_does_not_overwrite() {
    let mut call = pending_call();
    call.update_args(r#"{"task_id":"42"}"#);
    call.update_args(""); // 空字符串应被忽略
    assert_eq!(call.args_preview, r#"{"task_id":"42"}"#);
}
