use super::*;

#[test]
fn endpoint_drops_credentials_query_and_fragment() {
    assert_eq!(
        sanitize_endpoint("https://user:pass@example.com/v1/chat?api_key=secret#frag"),
        "https://example.com/v1/chat"
    );
    assert_eq!(sanitize_endpoint("not a url"), "<invalid-endpoint>");
}

#[test]
fn json_preview_redacts_nested_secret_fields() {
    let preview = sanitize_preview(
        r#"{"error":{"message":"bad","api_key":"sk-secret","nested":{"access_token":"token"}}}"#,
    );
    assert!(preview.contains("[REDACTED]"));
    assert!(!preview.contains("sk-secret"));
    assert!(!preview.contains("\"token\""));
}

#[test]
fn text_preview_redacts_common_inline_secrets_and_truncates() {
    let input = format!(
        "authorization Bearer-secret api_key=secret {}",
        "x".repeat(2_000)
    );
    let preview = sanitize_preview(&input);
    assert!(!preview.contains("Bearer-secret"));
    assert!(!preview.contains("api_key=secret"));
    assert!(preview.ends_with('…'));
}

/// Review finding #5: `sanitize_preview` truncates to `PREVIEW_LIMIT`
/// *characters* before attempting to parse/redact. For a **compact**
/// (no whitespace) JSON body whose total length exceeds the limit, the
/// char-level cut lands mid-document, so `serde_json::from_str` fails on
/// the now-invalid truncated JSON and the code falls back to
/// `redact_text`. But `redact_text` only recognizes secrets as
/// whitespace-delimited tokens matching `key=value` (e.g.
/// `api_key=secret`); a compact JSON body uses `"api_key":"value"`
/// (colon, not `=`, and no surrounding whitespace to split on), so the
/// secret survives untouched in the logged preview even though the
/// `api_key` field sits well within the first `PREVIEW_LIMIT`
/// characters of the raw body.
#[test]
fn compact_json_preview_over_limit_still_redacts_leading_api_key() {
    let secret = "sk-super-secret-0123456789";
    // Filler pushes the *overall* document past PREVIEW_LIMIT while the
    // `api_key` field itself sits near the very start, well inside the
    // truncation window.
    let filler = "y".repeat(2_000);
    let raw = format!(r#"{{"api_key":"{secret}","message":"error detail","filler":"{filler}"}}"#);
    assert!(
        raw.chars().count() > PREVIEW_LIMIT,
        "fixture must exceed the preview truncation limit"
    );
    let key_pos = raw.find("api_key").expect("fixture contains api_key");
    assert!(
        key_pos < PREVIEW_LIMIT,
        "api_key must sit inside the first PREVIEW_LIMIT characters"
    );

    let preview = sanitize_preview(&raw);

    assert!(
        !preview.contains(secret),
        "api_key leaked in a truncated compact-JSON preview even though the field was \
             well within the first {PREVIEW_LIMIT} characters of the raw body: {preview}"
    );
}
