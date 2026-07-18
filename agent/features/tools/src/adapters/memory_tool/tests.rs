use super::helpers::*;

#[test]
fn test_validate_content_normal() {
    assert!(validate_content("记住这个决策").is_ok());
}

#[test]
fn test_validate_content_empty() {
    assert!(validate_content("   ").is_err());
}

#[test]
fn test_validate_content_too_long() {
    let content = "x".repeat(MAX_CONTENT_CHARS + 1);
    assert!(validate_content(&content).is_err());
}

#[test]
fn test_parse_tags_normal() {
    let input = serde_json::json!({"tags": ["rust", "rust", " memory "]});
    let tags = parse_tags(&input).unwrap();

    assert_eq!(tags, vec!["memory", "rust"]);
}

#[test]
fn test_parse_tags_empty_array() {
    let input = serde_json::json!({"tags": []});
    let tags = parse_tags(&input).unwrap();

    assert!(tags.is_empty());
}

#[test]
fn test_parse_tags_invalid_item() {
    let input = serde_json::json!({"tags": [1]});

    assert!(parse_tags(&input).is_err());
}
