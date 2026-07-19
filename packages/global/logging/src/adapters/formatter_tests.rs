use super::*;
use serde_json::Value;

#[test]
fn diag_line_has_fourteen_fields() {
    let line = format_diag_json_line_from_parts("INFO", "cli::render", "frame");
    let value: Value = serde_json::from_str(&line).expect("valid json");
    let obj = value.as_object().expect("object");
    assert_eq!(obj.len(), 14);
    for key in [
        "ts",
        "boot_ts",
        "pid",
        "ver",
        "session",
        "chat",
        "turn",
        "request_id",
        "model",
        "provider",
        "role",
        "level",
        "target",
        "msg",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
    assert_eq!(obj["level"], "INFO");
    assert_eq!(obj["target"], "cli::render");
    assert_eq!(obj["msg"], "frame");
}

#[test]
fn explicit_context_formats_one_complete_snapshot() {
    let context = crate::domain::LogContext {
        session_id: Some("session-scope".to_string()),
        chat_id: Some("chat-scope".to_string()),
        turn: Some(0),
        request_id: Some("request-scope".to_string()),
        model: Some("model-scope".to_string()),
        provider: Some("provider-scope".to_string()),
        role: Some("role-scope".to_string()),
    };

    let line = format_diag_json_line_with_context("INFO", "aemeath:shared", "snapshot", &context);
    let value: Value = serde_json::from_str(&line).expect("valid json");
    assert_eq!(value["session"], "session-scope");
    assert_eq!(value["chat"], "chat-scope");
    assert_eq!(value["turn"], 0);
    assert_eq!(value["request_id"], "request-scope");
    assert_eq!(value["model"], "model-scope");
    assert_eq!(value["provider"], "provider-scope");
    assert_eq!(value["role"], "role-scope");
}

#[test]
fn missing_scope_uses_empty_context() {
    let line = format_diag_json_line_from_parts("INFO", "aemeath:shared", "no-scope");
    let value: Value = serde_json::from_str(&line).expect("valid json");
    assert_eq!(value["session"], "-");
    assert_eq!(value["role"], Value::Null);
}

#[test]
fn diag_line_is_compact_single_line() {
    let line = format_diag_json_line_from_parts("INFO", "test", "hello");
    assert!(!line.contains('\n'), "diag line must not contain newline");
}
