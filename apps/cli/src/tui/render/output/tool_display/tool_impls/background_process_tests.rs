//! Background Process 工具族显示注册测试（#1895）：registry 断言 +
//! header 定制 + typed result 解析渲染。

use crate::tui::render::output::tool_display::lookup_display;

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
                "goal": "ping -c 30",
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
                "goal": "ping",
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
    assert!(
        lines[0].contains("no deadline"),
        "无 deadline 显示占位：{}",
        lines[0]
    );
    assert!(
        !lines[0].contains("ms"),
        "Status 用户可见耗时 NEVER 含 ms：{}",
        lines[0]
    );
}

#[test]
fn status_result_formats_deadline_without_ms() {
    let display = lookup_display("BackgroundProcessStatus").unwrap();
    let result = content(serde_json::json!({
        "detail": {
            "summary": {
                "task_id": "bgp_deadline",
                "tool_name": "Bash",
                "state": "backgrounded",
                "goal": "long",
                "duration_ms": 850
            },
            "deadline_remaining_ms": 5000,
            "total_written_bytes": 10
        }
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].contains("deadline 5.00s"),
        "deadline 应走小数秒：{}",
        lines[0]
    );
    assert!(
        !lines[0].contains("ms"),
        "Status deadline NEVER 含 ms：{}",
        lines[0]
    );
    assert!(
        lines[0].contains("long"),
        "Status result MUST 含账本 summary：{}",
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

#[test]
fn watch_header_without_goal_falls_back_to_full_task_id() {
    let display = lookup_display("BackgroundProcessWatch").unwrap();
    let input = serde_json::json!({ "task_id": "bgp_07Xwatch1" });
    let header = display.format_header(&input, None);
    assert_eq!(
        header, "bgp_07Xwatch1",
        "无 goal 退化完整 task_id：{header}"
    );
    assert!(
        !header.contains("timeout="),
        "Watch header MUST NOT 展示入参 timeout：{header}"
    );
}

#[test]
fn watch_header_uses_input_goal_without_waiting_for_result() {
    use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
    let display = lookup_display("BackgroundProcessWatch").unwrap();
    // Runtime 已注入 goal；不等 typed result。
    let input = serde_json::json!({
        "task_id": "bgp_07dYn1F3S6a",
        "goal": "跑 ping 验收"
    });
    let header = display.format_header(&input, None);
    assert_eq!(header, "Watch · 跑 ping 验收");

    let payload = ToolResultPayload::new(
        String::new(),
        serde_json::json!({
            "outcome": "still_running",
            "detail": {
                "summary": {
                    "task_id": "bgp_07dYn1F3S6a",
                    "tool_name": "Bash",
                    "state": "backgrounded",
                    "goal": "不应覆盖 input goal",
                    "duration_ms": 5000
                },
                "deadline_remaining_ms": null,
                "total_written_bytes": 10
            },
            "log_tail": ""
        }),
        false,
        0,
    );
    let line = display.format_header_line_with_result(&input, Some(&payload), None);
    let with_result: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(
        with_result, "Watch · 跑 ping 验收",
        "有 result 仍只读 input.goal：{with_result}"
    );
    assert!(
        !with_result.contains("bgp_"),
        "有 goal 时 header NEVER 含 id：{with_result}"
    );
}

#[test]
fn watch_result_renders_outcome_not_json() {
    let display = lookup_display("BackgroundProcessWatch").unwrap();
    let result = content(serde_json::json!({
        "outcome": "still_running",
        "detail": {
            "summary": {
                "task_id": "bgp_w1",
                "tool_name": "Bash",
                "state": "backgrounded",
                "goal": "cargo test",
                "duration_ms": 5000
            },
            "deadline_remaining_ms": null,
            "total_written_bytes": 12
        },
        "log_tail": "watch-tail-line-a\nwatch-tail-line-b\n"
    }));
    let lines = display
        .format_result_lines(result.as_ref())
        .expect("typed 渲染产出");
    assert!(lines.len() >= 3, "log_tail + outcome 行：{lines:?}");
    let outcome_line = lines.last().expect("outcome 行");
    assert!(
        outcome_line.contains("still_running"),
        "outcome 应在末行以免 tail 裁掉：{outcome_line}"
    );
    assert!(outcome_line.contains("bgp_w1"));
    assert!(outcome_line.contains("backgrounded"));
    assert!(
        outcome_line.contains("5.00s"),
        "Watch duration 应走小数秒：{outcome_line}"
    );
    assert!(
        !outcome_line.contains("ms"),
        "Watch 用户可见耗时 NEVER 含 ms：{outcome_line}"
    );
    assert!(
        !outcome_line.starts_with('{'),
        "不渲染 JSON 原文：{outcome_line}"
    );
    assert!(
        lines.iter().any(|line| line.contains("watch-tail-line-a")),
        "应渲染 log_tail：{lines:?}"
    );
}
