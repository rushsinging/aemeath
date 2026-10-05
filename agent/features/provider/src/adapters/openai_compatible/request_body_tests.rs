//! `request_body.rs` 请求日志构造测试。
//!
//! 覆盖 debug 级请求摘要与 trace 级完整 body 两个 payload 构造函数：
//! 顶层字段名（400 排障的关键证据）、preview 截断、完整 body 保留与凭据不落盘。

use super::request_body::{build_request_body_log, build_request_log_summary};

fn sample_body() -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-6.1-sol",
        "input": [{"role": "user", "content": [{"type": "input_text", "text": "你好"}]}],
        "stream": true,
        "reasoning": {"effort": "xhigh"},
        "tools": [{"type": "function", "name": "Bash"}]
    })
}

#[test]
fn request_log_summary_reports_api_endpoint_bytes_and_top_level_keys() {
    let body = sample_body();
    let request_bytes = serde_json::to_string(&body).unwrap().len();

    let summary = build_request_log_summary(
        "responses_stream",
        "https://example.test/v1/responses",
        request_bytes,
        &body,
    );

    assert_eq!(summary["event_type"], "llm_request");
    assert_eq!(summary["api"], "responses_stream");
    assert_eq!(summary["endpoint"], "https://example.test/v1/responses");
    assert_eq!(summary["request_bytes"], request_bytes);
    let keys = summary["top_level_keys"]
        .as_array()
        .expect("top_level_keys must be an array");
    let keys: Vec<&str> = keys.iter().filter_map(|v| v.as_str()).collect();
    // serde_json Value::Object 默认 BTreeMap，keys 以字典序返回；
    // 排障需要的是顶层字段名集合本身，顺序不参与断言。
    assert_eq!(keys, vec!["input", "model", "reasoning", "stream", "tools"]);
}

#[test]
fn request_log_summary_truncates_preview_to_200_chars_with_total_length() {
    let long_text = "a".repeat(500);
    let body = serde_json::json!({"model": "m", "input": long_text});
    let request_bytes = serde_json::to_string(&body).unwrap().len();

    let summary = build_request_log_summary(
        "chat_completions_stream",
        "https://example.test/v1/chat",
        request_bytes,
        &body,
    );

    let preview = summary["preview"]
        .as_str()
        .expect("preview must be a string");
    assert_eq!(
        preview.chars().count(),
        200,
        "preview must be capped at 200 chars"
    );
    assert_eq!(
        summary["request_bytes"], request_bytes,
        "total size must accompany the truncated preview"
    );
}

#[test]
fn request_body_log_keeps_complete_body_for_trace_diagnosis() {
    let body = sample_body();

    let payload = build_request_body_log(
        "responses_stream",
        "https://example.test/v1/responses",
        &body,
    );

    assert_eq!(payload["event_type"], "llm_request_body");
    assert_eq!(payload["api"], "responses_stream");
    assert_eq!(
        payload["body"], body,
        "trace payload must carry the exact wire body"
    );
}

#[test]
fn request_logs_never_carry_credentials() {
    let body = sample_body();

    let summary = build_request_log_summary(
        "responses_stream",
        "https://example.test/v1/responses",
        128,
        &body,
    );
    let full = build_request_body_log(
        "responses_stream",
        "https://example.test/v1/responses",
        &body,
    );

    for payload in [summary, full] {
        let serialized = serde_json::to_string(&payload).unwrap();
        assert!(
            !serialized.contains("api_key")
                && !serialized.contains("Authorization")
                && !serialized.contains("Bearer"),
            "request logs must never embed header/credential fields: {serialized}"
        );
    }
}
