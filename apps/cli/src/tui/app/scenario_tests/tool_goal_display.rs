//! #1964 L4：工具卡片 Header 显示 goal；Background* 映射账本 summary。

use crate::tui::adapter::tui_runtime_event::{TuiRunContext, TuiRuntimeEvent, TuiToolCallStatus};

use super::super::testing::TuiScenarioHarness;

fn ctx() -> TuiRunContext {
    TuiRunContext {
        chat_id: "chat-goal".to_string(),
        run_id: "turn-goal".to_string(),
    }
}

fn screen_has(screen: &str, needle: &str) -> bool {
    screen.replace(' ', "").contains(&needle.replace(' ', ""))
}

#[test]
fn grep_tool_card_header_shows_goal_and_details_show_pattern() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    let id = "grep-goal-1".to_string();
    harness.runtime_event(TuiRuntimeEvent::ToolCallStarted {
        context: ctx(),
        id: id.clone(),
        provider_id: Some("provider-grep-1".into()),
        name: "Grep".into(),
        index: 0,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolCallStateChanged {
        context: ctx(),
        id: id.clone(),
        provider_id: Some("provider-grep-1".into()),
        name: "Grep".into(),
        index: 0,
        arguments: Some(serde_json::json!({
            "goal": "找 timeout 用法",
            "pattern": "timeout_secs",
            "path": "apps/cli"
        })),
        status: TuiToolCallStatus::Ready,
    });
    harness.render();
    let screen = harness.screen();
    assert!(
        screen_has(&screen, "找 timeout 用法"),
        "Grep header MUST 含 goal：
{screen}"
    );
    assert!(
        screen.contains("timeout_secs") || screen.contains("apps/cli"),
        "Grep Details MUST 含 pattern/path 载荷：
{screen}"
    );
    assert!(
        screen.contains("Search") || screen.contains("Grep"),
        "应渲染工具显示名：
{screen}"
    );

    harness.runtime_event(TuiRuntimeEvent::ToolResult {
        context: ctx(),
        id,
        provider_id: "provider-grep-1".into(),
        tool_name: "Grep".into(),
        output: "apps/cli/src/main.rs:1".into(),
        content: serde_json::json!({
            "matches": [],
            "total_matches": 0,
            "files": [{"file_path": "apps/cli/src/main.rs", "match_count": 1}],
            "output_mode": "files_with_matches"
        }),
        is_error: false,
        images: vec![],
        duration_ms: Some(850),
    });
    harness.render();
    let completed = harness.screen();
    assert!(
        screen_has(&completed, "找 timeout 用法"),
        "完成后 header 仍应保留 goal：
{completed}"
    );
    harness.assert_idle();
}

#[test]
fn background_list_and_watch_show_ledger_summary() {
    let mut harness = TuiScenarioHarness::new(120, 40);

    let list_id = "bgp-list-1".to_string();
    harness.runtime_event(TuiRuntimeEvent::ToolCallStarted {
        context: ctx(),
        id: list_id.clone(),
        provider_id: Some("provider-list".into()),
        name: "BackgroundProcessList".into(),
        index: 0,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolCallStateChanged {
        context: ctx(),
        id: list_id.clone(),
        provider_id: Some("provider-list".into()),
        name: "BackgroundProcessList".into(),
        index: 0,
        arguments: Some(serde_json::json!({"goal": "列后台"})),
        status: TuiToolCallStatus::Ready,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolResult {
        context: ctx(),
        id: list_id,
        provider_id: "provider-list".into(),
        tool_name: "BackgroundProcessList".into(),
        output: "listed".into(),
        content: serde_json::json!({
            "tasks": [{
                "task_id": "bgp_07dYn1F3S6a",
                "tool_name": "Bash",
                "state": "backgrounded",
                "summary": "Run · 跑 ping 验收",
                "duration_ms": 12000
            }]
        }),
        is_error: false,
        images: vec![],
        duration_ms: Some(12),
    });
    harness.render();
    let list_screen = harness.screen();
    assert!(
        screen_has(&list_screen, "Run · 跑 ping 验收"),
        "List result MUST 显示账本 summary：
{list_screen}"
    );

    let watch_id = "bgp-watch-1".to_string();
    harness.runtime_event(TuiRuntimeEvent::ToolCallStarted {
        context: ctx(),
        id: watch_id.clone(),
        provider_id: Some("provider-watch".into()),
        name: "BackgroundProcessWatch".into(),
        index: 1,
    });
    harness.runtime_event(TuiRuntimeEvent::ToolCallStateChanged {
        context: ctx(),
        id: watch_id.clone(),
        provider_id: Some("provider-watch".into()),
        name: "BackgroundProcessWatch".into(),
        index: 1,
        arguments: Some(serde_json::json!({
            "goal": "盯 ping",
            "task_id": "bgp_07dYn1F3S6a"
        })),
        status: TuiToolCallStatus::Ready,
    });
    harness.render();
    let waiting = harness.screen();
    assert!(
        screen_has(&waiting, "盯 ping"),
        "Watch 等待中 header 应用自身 goal：
{waiting}"
    );

    harness.runtime_event(TuiRuntimeEvent::ToolResult {
        context: ctx(),
        id: watch_id,
        provider_id: "provider-watch".into(),
        tool_name: "BackgroundProcessWatch".into(),
        output: "still_running".into(),
        content: serde_json::json!({
            "outcome": "still_running",
            "detail": {
                "summary": {
                    "task_id": "bgp_07dYn1F3S6a",
                    "tool_name": "Bash",
                    "state": "backgrounded",
                    "summary": "Run · 跑 ping 验收",
                    "duration_ms": 5000
                },
                "deadline_remaining_ms": null,
                "total_written_bytes": 64
            },
            "log_tail": "bytes from"
        }),
        is_error: false,
        images: vec![],
        duration_ms: Some(5000),
    });
    harness.render();
    let watch_screen = harness.screen();
    assert!(
        screen_has(&watch_screen, "Run · 跑 ping 验收"),
        "Watch header/result MUST 映射账本 summary：
{watch_screen}"
    );
    assert!(
        watch_screen.contains("Backend Process Watch") || watch_screen.contains("Watch"),
        "应显示 Watch 友好名：
{watch_screen}"
    );
    harness.assert_idle();
}
