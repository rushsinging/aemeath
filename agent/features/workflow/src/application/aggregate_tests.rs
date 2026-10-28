//! 生命周期状态机测试（设计文档 §11.2 E / G / D1–D2；#2085 TDD 红阶段）。

use super::*;
use crate::domain::tool_data::{AllowedAction, CreateWorkflowSpec, EvidenceKind, EvidenceRef};
use crate::domain::tool_error::WorkflowToolError;
use crate::domain::{ActiveMode, NodeKind, NodeStatus, Role, WorkflowStatus};

fn spec() -> CreateWorkflowSpec {
    CreateWorkflowSpec {
        name: "design".to_owned(),
        objective: "ship".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![
            crate::domain::NodeDefinition {
                node_id: "root".to_owned(),
                kind: NodeKind::Work,
                role: Some(Role::Planner),
                description: "plan".to_owned(),
                completion_criteria: None,
            },
            crate::domain::NodeDefinition {
                node_id: "done".to_owned(),
                kind: NodeKind::Complete,
                role: None,
                description: "finish".to_owned(),
                completion_criteria: None,
            },
            crate::domain::NodeDefinition {
                node_id: "extra".to_owned(),
                kind: NodeKind::Work,
                role: Some(Role::Implementer),
                description: "optional branch".to_owned(),
                completion_criteria: None,
            },
        ],
        edges: vec![
            crate::domain::EdgeDefinition {
                from_node_id: "root".to_owned(),
                to_node_id: "done".to_owned(),
            },
            crate::domain::EdgeDefinition {
                from_node_id: "root".to_owned(),
                to_node_id: "extra".to_owned(),
            },
            crate::domain::EdgeDefinition {
                from_node_id: "extra".to_owned(),
                to_node_id: "done".to_owned(),
            },
        ],
    }
}

fn aggregate() -> WorkflowAggregate {
    WorkflowAggregate::new(spec(), "wf_1", "2026-10-28T00:00:00Z").expect("valid spec")
}

/// E1：Executing 挂起后恢复回到 Executing。
#[test]
fn e1_executing_suspend_resume() {
    let mut agg = aggregate();
    assert_eq!(agg.status(), WorkflowStatus::Active);
    assert_eq!(agg.active_mode(), Some(ActiveMode::Executing));
    assert_eq!(agg.pending_mode(), None);

    agg.deactivate().expect("deactivate executing");
    assert_eq!(agg.status(), WorkflowStatus::Inactive);
    assert_eq!(agg.active_mode(), None);
    assert_eq!(agg.pending_mode(), None);

    agg.activate().expect("activate from none");
    assert_eq!(agg.status(), WorkflowStatus::Active);
    assert_eq!(agg.active_mode(), Some(ActiveMode::Executing));
}

/// E2：Replanning 挂起保留 pending_mode，恢复回到 Replanning 而非 Executing。
#[test]
fn e2_replanning_suspend_resume_keeps_pending_mode() {
    let mut agg = aggregate();
    agg.enter_replanning();
    assert_eq!(agg.active_mode(), Some(ActiveMode::Replanning));

    agg.deactivate().expect("deactivate replanning");
    assert_eq!(agg.status(), WorkflowStatus::Inactive);
    assert_eq!(agg.active_mode(), None);
    assert_eq!(agg.pending_mode(), Some(ActiveMode::Replanning));

    agg.activate().expect("activate from replanning");
    assert_eq!(agg.status(), WorkflowStatus::Active);
    assert_eq!(
        agg.active_mode(),
        Some(ActiveMode::Replanning),
        "MUST restore Replanning, never Executing"
    );
}

/// E3：Inactive 拒绝 complete / fail / node 操作。
#[test]
fn e3_inactive_rejects_progression() {
    let mut agg = aggregate();
    agg.deactivate().expect("suspend");
    assert!(matches!(
        agg.complete("done"),
        Err(WorkflowToolError::WorkflowNotActive { .. })
    ));
    assert!(matches!(
        agg.fail("no reason"),
        Err(WorkflowToolError::WorkflowNotActive { .. })
    ));
    // 状态不变。
    assert_eq!(agg.status(), WorkflowStatus::Inactive);
}

/// E4：cancel 在 Executing / Replanning / Inactive 均可达，终态不可逆。
#[test]
fn e4_cancel_from_three_states_then_terminal() {
    for setup in ["executing", "replanning", "inactive"] {
        let mut agg = aggregate();
        if setup == "replanning" {
            agg.enter_replanning();
        } else if setup == "inactive" {
            agg.deactivate().expect("suspend");
        }
        agg.cancel("user gave up").expect("cancel must work");
        assert_eq!(agg.status(), WorkflowStatus::Cancelled);
        assert!(matches!(
            agg.activate(),
            Err(WorkflowToolError::WorkflowAlreadyTerminal { .. })
        ));
        assert!(matches!(
            agg.cancel("again"),
            Err(WorkflowToolError::WorkflowAlreadyTerminal { .. })
        ));
        assert_eq!(agg.status(), WorkflowStatus::Cancelled);
    }
}

/// E5：complete 条件不足——CompleteNode 未完成或仍有 Active Node。
#[test]
fn e5_complete_requires_conditions() {
    // root 仍 Active → 拒绝。
    let mut agg = aggregate();
    assert!(matches!(
        agg.complete("summary"),
        Err(WorkflowToolError::CompletionConditionsUnsatisfied { .. })
    ));
    assert_eq!(agg.status(), WorkflowStatus::Active);
}

/// E6：CompleteNode 已完成且无 Active Node 时 complete 成功，允许 Pending 残留。
#[test]
fn e6_complete_allows_pending_residue() {
    let mut agg = aggregate();
    agg.mark_node_completed("root").expect("root completed");
    agg.mark_node_completed("done").expect("complete node done");
    // extra 仍 Pending（从未激活）——允许残留。
    assert_eq!(agg.node_status("extra"), Some(NodeStatus::Pending));
    agg.complete("delivered").expect("complete allowed");
    assert_eq!(agg.status(), WorkflowStatus::Completed);
    // 查询视图如实显示 Pending 残留。
    let pending: Vec<String> = agg
        .node_ids_with_status(NodeStatus::Pending)
        .cloned()
        .collect();
    assert_eq!(pending, vec!["extra".to_owned()]);
}

/// E8：fail 仅 Replanning 可达；Completed / Cancelled / Failed 均不可逆。
#[test]
fn e8_fail_only_from_replanning_and_terminal_irreversible() {
    let mut agg = aggregate();
    assert!(matches!(
        agg.fail("not in replanning"),
        Err(WorkflowToolError::WorkflowNotReplanning { .. })
    ));
    agg.enter_replanning();
    agg.fail("cannot continue").expect("fail from replanning");
    assert_eq!(agg.status(), WorkflowStatus::Failed);
    assert!(matches!(
        agg.activate(),
        Err(WorkflowToolError::WorkflowAlreadyTerminal { .. })
    ));
    assert_eq!(agg.status(), WorkflowStatus::Failed);
}

/// D2：Replanning 中 complete 一律拒绝。
#[test]
fn d2_complete_rejected_in_replanning() {
    let mut agg = aggregate();
    agg.enter_replanning();
    assert!(matches!(
        agg.complete("summary"),
        Err(WorkflowToolError::WorkflowReplanning { .. })
    ));
    assert_eq!(agg.active_mode(), Some(ActiveMode::Replanning));
}

/// G1：allowed_actions 与 §5.3 对照表完全一致。
#[test]
fn g1_allowed_actions_matrix() {
    let mut agg = aggregate();
    assert_eq!(
        agg.allowed_actions(),
        vec![
            AllowedAction::Deactivate,
            AllowedAction::Complete,
            AllowedAction::Cancel,
            AllowedAction::NodeComplete,
            AllowedAction::NodeFail,
        ]
    );
    agg.enter_replanning();
    assert_eq!(
        agg.allowed_actions(),
        vec![
            AllowedAction::Replan,
            AllowedAction::Fail,
            AllowedAction::Cancel,
            AllowedAction::Deactivate,
        ]
    );
    agg.deactivate().expect("suspend");
    assert_eq!(
        agg.allowed_actions(),
        vec![AllowedAction::Activate, AllowedAction::Cancel]
    );
    agg.activate().expect("back to replanning");
    agg.fail("done").expect("terminal");
    assert!(agg.allowed_actions().is_empty());
}

/// G2：allowed_actions 只是提示——complete 在列表中但条件不满足仍拒绝。
#[test]
fn g2_allowed_actions_do_not_grant_authorization() {
    let mut agg = aggregate();
    assert!(agg.allowed_actions().contains(&AllowedAction::Complete));
    assert!(matches!(
        agg.complete("summary"),
        Err(WorkflowToolError::CompletionConditionsUnsatisfied { .. })
    ));
}

/// create：Kernel 校验失败拒绝且不留半成品。
#[test]
fn create_rejects_invalid_graph() {
    let mut bad = spec();
    bad.nodes.retain(|node| node.kind != NodeKind::Complete);
    assert!(matches!(
        WorkflowAggregate::new(bad, "wf_bad", "t"),
        Err(WorkflowToolError::GraphInvariantViolation { .. })
    ));
}

/// create：root 成为唯一初始 Active Node。
#[test]
fn create_initializes_root_active() {
    let agg = aggregate();
    assert_eq!(agg.node_status("root"), Some(NodeStatus::Active));
    assert_eq!(agg.node_status("done"), Some(NodeStatus::Pending));
    assert_eq!(agg.node_status("extra"), Some(NodeStatus::Pending));
}

/// summary/结论/证据随节点保存（供 #2086 消费的受控入口语义）。
#[test]
fn node_conclusion_and_evidence_recorded() {
    let mut agg = aggregate();
    let evidence = vec![EvidenceRef {
        fact_id: "fact_1".to_owned(),
        kind: EvidenceKind::ToolResult,
    }];
    agg.complete_node_with("root", "plan ready".to_owned(), evidence.clone())
        .expect("complete node");
    assert_eq!(agg.node_status("root"), Some(NodeStatus::Completed));
    assert_eq!(agg.node_conclusion("root"), Some("plan ready"));
    assert_eq!(agg.node_evidence("root"), Some(evidence));
}
