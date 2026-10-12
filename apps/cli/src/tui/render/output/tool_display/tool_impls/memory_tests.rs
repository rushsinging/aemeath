//! Memory 工具族显示注册测试：registry 断言 + header 定制 + typed result 解析渲染。

use crate::tui::render::output::tool_display::lookup_display;

fn content(value: serde_json::Value) -> Option<serde_json::Value> {
    Some(value)
}

#[test]
fn test_lookup_display_finds_memory_family() {
    for name in [
        "MemoryAdd",
        "MemorySearch",
        "MemoryList",
        "MemoryUpdate",
        "MemoryDelete",
    ] {
        let display = lookup_display(name);
        assert!(display.is_some(), "{name} 应注册 display");
        assert_eq!(display.unwrap().name(), name);
    }
}

#[test]
fn add_header_carries_category_layer_and_content() {
    let display = lookup_display("MemoryAdd").unwrap();
    let input = serde_json::json!({
        "content": "remember this fact about timeouts",
        "layer": "project",
        "category": "pitfall"
    });
    let header = display.format_header(&input, None);
    assert!(
        header.contains("pitfall"),
        "Add header 含 category：{header}"
    );
    assert!(header.contains("project"), "Add header 含 layer：{header}");
    assert!(
        header.contains("remember this fact"),
        "Add header 含 content：{header}"
    );
}

#[test]
fn search_header_carries_query() {
    let display = lookup_display("MemorySearch").unwrap();
    let input = serde_json::json!({
        "query": "background process watch",
        "layer": "project",
        "category": "decision"
    });
    let header = display.format_header(&input, None);
    assert!(
        header.contains("background process watch"),
        "Search header 含 query：{header}"
    );
    assert!(
        header.contains("project"),
        "Search header 含 layer：{header}"
    );
    assert!(
        header.contains("decision"),
        "Search header 含 category：{header}"
    );
}

#[test]
fn list_header_carries_optional_layer() {
    let display = lookup_display("MemoryList").unwrap();
    let input = serde_json::json!({ "layer": "global" });
    let header = display.format_header(&input, None);
    assert!(header.contains("global"), "List header 含 layer：{header}");
}

#[test]
fn list_header_carries_offset_and_limit() {
    let display = lookup_display("MemoryList").unwrap();
    let input = serde_json::json!({ "layer": "project", "offset": 20, "limit": 10 });
    let header = display.format_header(&input, None);
    assert!(
        header.contains("offset=20"),
        "List header 含 offset：{header}"
    );
    assert!(
        header.contains("limit=10"),
        "List header 含 limit：{header}"
    );
}

#[test]
fn update_header_carries_id_and_status() {
    let display = lookup_display("MemoryUpdate").unwrap();
    let input = serde_json::json!({
        "id": "01a125a4-5c45-7342-9aec-cd7798de96d8",
        "status": "archive"
    });
    let header = display.format_header(&input, None);
    assert!(
        header.contains("01a125a4-5c45-7342-9aec-cd7798de96d8"),
        "Update header 含 id：{header}"
    );
    assert!(
        header.contains("archive"),
        "Update header 含 status：{header}"
    );
}

#[test]
fn delete_header_carries_id() {
    let display = lookup_display("MemoryDelete").unwrap();
    let input = serde_json::json!({ "id": "mem_07aMXw8QI08" });
    let header = display.format_header(&input, None);
    assert!(
        header.contains("mem_07aMXw8QI08"),
        "Delete header 含 id：{header}"
    );
}

#[test]
fn list_result_renders_typed_lines_not_json() {
    let display = lookup_display("MemoryList").unwrap();
    let result = content(serde_json::json!({
        "action": "list",
        "entries": [
            {
                "id": "mem_1",
                "content": "Watch 必须强制前台",
                "layer": "project",
                "category": "decision",
                "tags": ["1931"],
                "pinned": true,
                "outdated": false,
                "ttl_expired": false,
                "superseded_by": null,
                "evidence": []
            }
        ]
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("mem_1"), "逐行摘要含 id：{}", lines[0]);
    assert!(lines[0].contains("project"));
    assert!(lines[0].contains("decision"));
    assert!(lines[0].contains("pinned"));
    assert!(lines[0].contains("Watch 必须强制前台"));
    assert!(!lines[0].starts_with('{'), "不渲染 JSON 原文：{}", lines[0]);
}

#[test]
fn list_result_empty_shows_placeholder() {
    let display = lookup_display("MemoryList").unwrap();
    let result = content(serde_json::json!({
        "action": "list",
        "entries": []
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines, vec!["（无条目）".to_string()]);
}

#[test]
fn list_result_appends_has_more_hint() {
    let display = lookup_display("MemoryList").unwrap();
    let result = content(serde_json::json!({
        "action": "list",
        "entries": [{
            "id": "mem_page",
            "content": "paged entry",
            "layer": "project",
            "category": "fact",
            "tags": [],
            "pinned": false,
            "outdated": false,
            "ttl_expired": false,
            "superseded_by": null,
            "evidence": []
        }],
        "total": 40,
        "offset": 0,
        "limit": 20,
        "has_more": true
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert!(lines.iter().any(|line| line.contains("mem_page")));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("has_more") && line.contains("next_offset=20")),
        "has_more 行应出现：{lines:?}"
    );
}

#[test]
fn search_result_renders_hits_not_json() {
    let display = lookup_display("MemorySearch").unwrap();
    let result = content(serde_json::json!({
        "action": "search",
        "hits": [
            {
                "id": "mem_hit",
                "content": "auto compact 估算口径",
                "layer": "project",
                "category": "decision",
                "tags": [],
                "pinned": false,
                "location": "active",
                "outdated": false,
                "ttl_expired": false,
                "superseded_by": null,
                "evidence": [],
                "relevance": 9.5
            }
        ]
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("mem_hit"));
    assert!(lines[0].contains("active"));
    assert!(lines[0].contains("9.5") || lines[0].contains("9.50"));
    assert!(lines[0].contains("auto compact"));
    assert!(!lines[0].starts_with('{'), "不渲染 JSON 原文：{}", lines[0]);
}

#[test]
fn search_result_empty_shows_placeholder() {
    let display = lookup_display("MemorySearch").unwrap();
    let result = content(serde_json::json!({
        "action": "search",
        "hits": []
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines, vec!["（无命中）".to_string()]);
}

#[test]
fn add_result_renders_action_and_id() {
    let display = lookup_display("MemoryAdd").unwrap();
    let result = content(serde_json::json!({
        "action": "added",
        "id": "mem_new"
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("added"));
    assert!(lines[0].contains("mem_new"));
    assert!(!lines[0].starts_with('{'));
}

#[test]
fn add_needs_eviction_renders_candidates() {
    let display = lookup_display("MemoryAdd").unwrap();
    let result = content(serde_json::json!({
        "action": "needs_eviction",
        "eviction_candidates": [
            {
                "id": "mem_old",
                "content": "旧条目待淘汰",
                "layer": "project",
                "category": "fact",
                "tags": [],
                "pinned": false,
                "outdated": true,
                "ttl_expired": false,
                "confirmation_count": 1,
                "last_confirmed_at": 0,
                "eviction_score": 42,
                "eviction_reason": "outdated"
            }
        ]
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert!(!lines.is_empty());
    assert!(lines.iter().any(|line| line.contains("mem_old")));
    assert!(lines.iter().any(|line| line.contains("outdated")));
    assert!(lines.iter().all(|line| !line.starts_with('{')));
}

#[test]
fn update_result_renders_action_and_id() {
    let display = lookup_display("MemoryUpdate").unwrap();
    let result = content(serde_json::json!({
        "action": "pin",
        "id": "mem_pin"
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("pin"));
    assert!(lines[0].contains("mem_pin"));
}

#[test]
fn delete_result_renders_action_and_id() {
    let display = lookup_display("MemoryDelete").unwrap();
    let result = content(serde_json::json!({
        "action": "delete",
        "id": "mem_del"
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("delete"));
    assert!(lines[0].contains("mem_del"));
}
