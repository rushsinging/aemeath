//! ToolSearch 显示注册与 typed result 测试。

use crate::tui::render::output::tool_display::lookup_display;

fn content(value: serde_json::Value) -> Option<serde_json::Value> {
    Some(value)
}

#[test]
fn lookup_display_finds_tool_search() {
    let display = lookup_display("ToolSearch").unwrap();
    assert_eq!(display.name(), "ToolSearch");
}

#[test]
fn tool_search_header_carries_query() {
    let display = lookup_display("ToolSearch").unwrap();
    let header = display.format_header(&serde_json::json!({ "query": "memory" }), None);
    assert!(header.contains("memory"), "header: {header}");
}

#[test]
fn tool_search_result_renders_summary_without_schema() {
    let display = lookup_display("ToolSearch").unwrap();
    let lines = display
        .format_result_lines(
            content(serde_json::json!({
                "tools": [
                    {
                        "name": "MemorySearch",
                        "description": "Search persistent memories by keywords",
                        "input_schema": { "type": "object", "properties": { "query": {} } },
                        "is_read_only": true
                    }
                ]
            }))
            .as_ref(),
        )
        .expect("typed 产出");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("tool"));
    assert!(lines[0].contains("MemorySearch"));
    assert!(lines[0].contains("ro"));
    assert!(lines[0].contains("Search persistent"));
    assert!(!lines[0].contains("input_schema"));
    assert!(!lines[0].starts_with('{'));
}

#[test]
fn tool_search_empty_shows_placeholder() {
    let display = lookup_display("ToolSearch").unwrap();
    let lines = display
        .format_result_lines(content(serde_json::json!({ "tools": [] })).as_ref())
        .expect("typed 产出");
    assert_eq!(lines, vec!["（无匹配）".to_string()]);
}
