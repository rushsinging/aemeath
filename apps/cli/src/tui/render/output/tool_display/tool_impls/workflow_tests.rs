//! Workflow 工具族 TUI 显示契约测试：友好名 / header / typed result 行 /
//! 关键状态行置末（tail 截断保结论）。

use super::super::super::tool_display::registry::lookup_display;
use serde_json::json;

fn display(name: &str) -> &'static dyn crate::tui::render::output::tool_display::ToolDisplay {
    lookup_display(name).expect("workflow tool display registered")
}

#[test]
fn six_workflow_displays_registered() {
    for name in [
        "workflow_create",
        "workflow_list",
        "workflow_query",
        "workflow_update",
        "workflow_node_update",
        "workflow_replan",
    ] {
        assert!(lookup_display(name).is_some(), "{name} 未注册");
    }
}

#[test]
fn create_header_uses_friendly_name_and_result_ends_with_status() {
    let tool = display("workflow_create");
    let header = tool.format_header(&json!({ "name": "验收A" }), None);
    assert_eq!(header, "Create Workflow 验收A");
    let lines = tool
        .format_result_lines(Some(&json!({
            "workflow_id": "01a19272-4f99",
            "status": "active",
            "active_mode": "executing",
            "node_count": 3
        })))
        .expect("result lines");
    assert_eq!(
        lines.last().unwrap(),
        "active (executing)",
        "状态行必须在末行"
    );
}

#[test]
fn list_renders_per_workflow_summary_and_total_tail() {
    let tool = display("workflow_list");
    let header = tool.format_header(&json!({ "status": "failed" }), None);
    assert_eq!(header, "Workflows [failed]");
    let lines = tool
        .format_result_lines(Some(&json!({
            "workflows": [{
                "workflow_id": "01a19272-d123",
                "name": "验收B",
                "status": "failed",
                "active_mode": null,
                "pending_mode": null,
                "node_summary": { "pending": 1, "active": 0, "completed": 0, "failed": 1, "stopped": 0 }
            }],
            "total": 1
        })))
        .expect("result lines");
    assert!(lines[0].starts_with("failed 01a1927"), "{}", lines[0]);
    assert!(lines[0].contains("验收B"));
    assert_eq!(lines.last().unwrap(), "total 1");
}

#[test]
fn node_update_fail_keeps_replanning_last() {
    let tool = display("workflow_node_update");
    let header = tool.format_header(
        &json!({ "workflow_id": "wf", "node_id": "root2", "action": "fail" }),
        None,
    );
    assert_eq!(header, "Workflow Node fail root2");
    let lines = tool
        .format_result_lines(Some(&json!({
            "node_id": "root2",
            "node_status": "failed",
            "status": "active",
            "active_mode": "replanning",
            "activated_node_ids": [],
            "stopped_node_ids": ["peer"]
        })))
        .expect("result lines");
    assert_eq!(lines[0], "root2 is failed");
    assert_eq!(
        lines.last().unwrap(),
        "active (replanning)",
        "Replanning 转换必须置于末行"
    );
}

#[test]
fn query_renders_node_rows_allowed_actions_and_status_tail() {
    let tool = display("workflow_query");
    let lines = tool
        .format_result_lines(Some(&json!({
            "status": "active",
            "active_mode": "replanning",
            "nodes": [
                { "node_id": "root", "status": "failed", "kind": "work", "conclusion": "无法继续" },
                { "node_id": "next", "status": "pending", "kind": "work", "conclusion": null }
            ],
            "allowed_actions": ["replan", "fail", "cancel", "deactivate"]
        })))
        .expect("result lines");
    assert!(lines.iter().any(|l| l.starts_with("failed root/work")));
    assert!(lines.iter().any(|l| l.starts_with("pending next/work")));
    assert!(lines.iter().any(|l| l.starts_with("allowed: replan")));
    assert_eq!(lines.last().unwrap(), "active (replanning)");
}

#[test]
fn update_header_includes_action_and_short_id() {
    let tool = display("workflow_update");
    let header = tool.format_header(
        &json!({ "workflow_id": "01a19272-4f99-78e3", "action": "deactivate" }),
        None,
    );
    assert_eq!(header, "Workflow State deactivate 01a19272");
    let lines = tool
        .format_result_lines(Some(&json!({
            "status": "inactive",
            "active_mode": null,
            "pending_mode": "replanning"
        })))
        .expect("result lines");
    assert_eq!(lines.last().unwrap(), "inactive (pending=replanning)");
}
