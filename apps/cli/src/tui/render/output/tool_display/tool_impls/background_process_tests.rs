//! Background Process 工具族显示注册测试（#1895）：registry 断言 +
//! header 定制 + typed result 解析渲染。

use crate::tui::render::output::tool_display::{lookup_display, ToolDisplay};

fn content(value: serde_json::Value) -> Option<serde_json::Value> {
    Some(value)
}

#[test]
fn test_lookup_display_finds_background_process_family() {
    for name in [
        "BackgroundProcessList",
        "BackgroundProcessStatus",
        "BackgroundProcessLogs",
        "BackgroundProcessStop",
    ] {
        let display = lookup_display(name);
        assert!(display.is_some(), "{name} 应注册 display");
        assert_eq!(display.unwrap().name(), name);
    }
}

#[test]
fn status_header_carries_task_id() {
    let display = lookup_display("BackgroundProcessStatus").unwrap();
    let input = serde_json::json!({ "task_id": "bgp_07Xabc123" });
    let header = display.format_header(&input, None);
    assert!(
        header.contains("bgp_07Xabc123"),
        "Status header 携带 task id：{header}"
    );
}

#[test]
fn logs_header_carries_task_id() {
    let display = lookup_display("BackgroundProcessLogs").unwrap();
    let input = serde_json::json!({ "task_id": "bgp_07Xdef456", "cursor": 0 });
    let header = display.format_header(&input, None);
    assert!(header.contains("bgp_07Xdef456"), "Logs header：{header}");
}

#[test]
fn stop_header_is_dedicated_and_carries_task_id() {
    let display = lookup_display("BackgroundProcessStop").unwrap();
    let input = serde_json::json!({ "task_id": "bgp_07Xghi789" });
    let header = display.format_header(&input, None);
    assert!(
        header.contains("bgp_07Xghi789"),
        "Stop 有自己的 header 且带 task id：{header}"
    );
}

#[test]
fn list_result_renders_typed_lines_not_json() {
    let display = lookup_display("BackgroundProcessList").unwrap();
    let result = content(serde_json::json!({
        "tasks": [
            {
                "task_id": "bgp_1",
                "tool_name": "Bash",
                "state": "backgrounded",
                "summary": "ping -c 30",
                "duration_ms": 12000
            }
        ]
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("bgp_1"), "逐行摘要含 id：{}`", lines[0]);
    assert!(lines[0].contains("Bash"));
    assert!(lines[0].contains("backgrounded"));
    assert!(!lines[0].starts_with('{'), "不渲染 JSON 原文：{}", lines[0]);
}

#[test]
fn logs_result_renders_multiline_text_verbatim() {
    let display = lookup_display("BackgroundProcessLogs").unwrap();
    let result = content(serde_json::json!({
        "log": { "text": "line-1\nline-2\nline-3", "cursor": 24, "total_written": 24 }
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(
        lines,
        vec!["line-1", "line-2", "line-3"],
        "多行原文按行渲染"
    );
}

#[test]
fn stop_result_renders_signal_state() {
    let display = lookup_display("BackgroundProcessStop").unwrap();
    let result = content(serde_json::json!({
        "task_id": "bgp_2",
        "stop": { "signal_sent": true, "state": "backgrounded" }
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines, vec!["signal_sent → backgrounded"]);
}

#[test]
fn status_result_renders_detail_with_log_bytes() {
    let display = lookup_display("BackgroundProcessStatus").unwrap();
    let result = content(serde_json::json!({
        "detail": {
            "summary": {
                "task_id": "bgp_3",
                "tool_name": "Bash",
                "state": "succeeded",
                "summary": "ping",
                "duration_ms": 30000
            },
            "deadline_remaining_ms": null,
            "total_written_bytes": 1783
        }
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].contains("log bytes 1783"),
        "详情含日志字节数：{}",
        lines[0]
    );
}

#[test]
fn empty_list_result_shows_placeholder() {
    let display = lookup_display("BackgroundProcessList").unwrap();
    let result = content(serde_json::json!({ "tasks": [] }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染");
    assert_eq!(lines, vec!["（无后台进程）"]);
}
