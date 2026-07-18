use super::extract::{extract_page, ExtractOptions};
use super::WebFetchTool;
use crate::domain::TypedTool;

#[test]
fn test_extract_title_and_markdown() {
    let html = r#"<!DOCTYPE html>
<html>
<head><title>Hello Page</title></head>
<body>
<nav>skip me</nav>
<main>
<h1>Hello</h1>
<p>World <a href="/relative">link</a></p>
</main>
<footer>foot</footer>
</body>
</html>"#;

    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 1_000_000,
        max_links: 50,
    };
    let result = extract_page(html, opts).unwrap();
    assert_eq!(result.title, "Hello Page");
    assert!(result.markdown.contains("Hello"));
    assert!(result
        .links
        .contains(&"https://example.com/relative".to_string()));
}

#[test]
fn test_fallback_to_body_when_no_main_or_article() {
    let html = r#"<html><head><title>No Main</title></head><body><p>content</p></body></html>"#;
    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 1_000_000,
        max_links: 50,
    };
    let result = extract_page(html, opts).unwrap();
    assert_eq!(result.title, "No Main");
    assert!(result.markdown.contains("content"));
}

#[test]
fn test_links_deduplicated_and_limited() {
    let html = r#"<html><body>
<a href="https://a.com/1">a</a>
<a href="https://a.com/1">a again</a>
<a href="https://a.com/2">b</a>
</body></html>"#;
    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 1_000_000,
        max_links: 2,
    };
    let result = extract_page(html, opts).unwrap();
    assert_eq!(result.links.len(), 2);
    assert!(result.links.contains(&"https://a.com/1".to_string()));
    assert!(result.links.contains(&"https://a.com/2".to_string()));
}

#[test]
fn test_exceeds_max_size_returns_error() {
    let html = "<html><body><p>hi</p></body></html>";
    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 10,
        max_links: 50,
    };
    assert!(extract_page(html, opts).is_err());
}

#[test]
fn test_non_html_content_not_parsed() {
    let text = "plain text content";
    let opts = ExtractOptions {
        base_url: "https://example.com",
        max_content_bytes: 1_000_000,
        max_links: 50,
    };
    let result = extract_page(text, opts).unwrap();
    assert!(result.markdown.contains("plain text content"));
    assert!(result.links.is_empty());
}

#[test]
fn test_web_fetch_result_schema_includes_links() {
    let tool = WebFetchTool;
    let schema = tool.data_schema();
    let properties = schema.get("properties").unwrap();
    assert!(properties.get("links").is_some());
}
