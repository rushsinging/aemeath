use crate::domain::redaction::{redact_json_value, redact_sensitive_text};

#[test]
fn redacts_sk_tokens() {
    let text = "K=\"sk-123374abd15b4964-0b7ae2-3cd66664\"; curl";
    let redacted = redact_sensitive_text(text);
    assert!(!redacted.contains("123374"), "{redacted}");
    assert!(redacted.contains("sk-***"), "{redacted}");
}

#[test]
fn redacts_bearer_tokens() {
    let redacted = redact_sensitive_text("Authorization: Bearer abc.def-123");
    assert!(!redacted.contains("abc.def-123"), "{redacted}");
    assert!(redacted.contains("Bearer ***"), "{redacted}");
}

#[test]
fn leaves_normal_text_unchanged() {
    assert_eq!(
        redact_sensitive_text("cargo test --lib harness"),
        "cargo test --lib harness"
    );
}

#[test]
fn recursively_redacts_json_strings() {
    let mut value = serde_json::json!({
        "signal_id": "retry_pattern:Bash:{\"command\":\"curl -H sk-secret\"}",
        "nested": ["Bearer token-1", 3]
    });
    redact_json_value(&mut value);
    let text = value.to_string();
    assert!(!text.contains("sk-secret"), "{text}");
    assert!(!text.contains("token-1"), "{text}");
    assert!(text.contains("sk-***"), "{text}");
}
