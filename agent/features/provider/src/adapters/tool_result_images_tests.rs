use super::*;

#[test]
fn extracts_images_from_mixed_content_array() {
    let content = serde_json::json!([
        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "QUJD"}},
        {"type": "text", "text": "Image: /tmp/a.png"},
    ]);
    assert_eq!(
        extract_tool_result_images(&content),
        vec![("image/png".to_string(), "QUJD".to_string())]
    );
}

#[test]
fn plain_string_content_yields_no_images() {
    assert!(extract_tool_result_images(&serde_json::json!("plain")).is_empty());
}

#[test]
fn malformed_image_block_is_skipped_without_panic() {
    let content = serde_json::json!([
        {"type": "image"},
        {"type": "image", "source": {"data": "QUJD"}},
        {"type": "image", "source": {"media_type": "image/jpeg", "data": "REVG"}},
    ]);
    assert_eq!(
        extract_tool_result_images(&content),
        vec![("image/jpeg".to_string(), "REVG".to_string())]
    );
}

#[test]
fn extracts_text_from_array_and_string_content() {
    let array = serde_json::json!([
        {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "QUJD"}},
        {"type": "text", "text": "first"},
        {"type": "text", "text": "second"},
    ]);
    assert_eq!(extract_tool_result_text(&array), "firstsecond");
    assert_eq!(
        extract_tool_result_text(&serde_json::json!("plain")),
        "plain"
    );
}
