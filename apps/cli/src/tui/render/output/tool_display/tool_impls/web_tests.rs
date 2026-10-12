//! WebFetch / WebSearch 显示注册与 typed result 测试。

use crate::tui::render::output::tool_display::lookup_display;

fn content(value: serde_json::Value) -> Option<serde_json::Value> {
    Some(value)
}

#[test]
fn lookup_display_finds_web_family() {
    for name in ["WebFetch", "WebSearch"] {
        let display = lookup_display(name);
        assert!(display.is_some(), "{name} 应注册 display");
        assert_eq!(display.unwrap().name(), name);
    }
}

#[test]
fn web_search_header_carries_query() {
    let display = lookup_display("WebSearch").unwrap();
    let header = display.format_header(
        &serde_json::json!({ "query": "rust worktree", "limit": 5 }),
        None,
    );
    assert!(header.contains("rust worktree"), "header: {header}");
}

#[test]
fn web_search_result_renders_hits_not_json() {
    let display = lookup_display("WebSearch").unwrap();
    let lines = display
        .format_result_lines(
            content(serde_json::json!({
                "results": [
                    {
                        "title": "Rust Book",
                        "url": "https://doc.rust-lang.org/book/",
                        "snippet": "The Rust Programming Language"
                    }
                ]
            }))
            .as_ref(),
        )
        .expect("typed 产出");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("Rust Book"));
    assert!(lines[0].contains("https://doc.rust-lang.org/book/"));
    assert!(!lines[0].starts_with('{'));
}

#[test]
fn web_search_empty_shows_placeholder() {
    let display = lookup_display("WebSearch").unwrap();
    let lines = display
        .format_result_lines(content(serde_json::json!({ "results": [] })).as_ref())
        .expect("typed 产出");
    assert_eq!(lines, vec!["（无结果）".to_string()]);
}

#[test]
fn web_fetch_result_summarizes_instead_of_full_markdown() {
    let display = lookup_display("WebFetch").unwrap();
    let long_body = format!("# Title\n\n{}\n\nmore", "x".repeat(4000));
    let lines = display
        .format_result_lines(
            content(serde_json::json!({
                "url": "https://example.com",
                "title": "Example Domain",
                "content": long_body,
                "truncated": true,
                "links": ["https://a", "https://b"]
            }))
            .as_ref(),
        )
        .expect("typed 产出");
    assert!(lines.iter().any(|line| line.contains("Example Domain")));
    assert!(lines.iter().any(|line| line.contains("truncated")));
    assert!(lines.iter().any(|line| line.contains("links: 2")));
    assert!(
        lines
            .iter()
            .all(|line| line.len() < 200 || line.chars().count() <= 130),
        "不应刷全文：{lines:?}"
    );
    assert!(lines.iter().all(|line| !line.starts_with('{')), "{lines:?}");
}
