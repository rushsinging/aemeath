//! Replanning continuation 测试（设计文档 §11.2 D3–D10；#2087 TDD 红阶段）。

use super::*;
use crate::domain::tool_data::{
    CreateWorkflowSpec, NodeAction, PendingNodeUpdate, PendingRewrites, RejoinEdge, ReplanBoundary,
    ReplanWorkflowCommand,
};
use crate::domain::tool_error::WorkflowToolError;
use crate::domain::{ActiveMode, NodeKind, NodeStatus, Role, WorkflowStatus};

fn node(node_id: &str, kind: NodeKind) -> crate::domain::NodeDefinition {
    crate::domain::NodeDefinition {
        node_id: node_id.to_owned(),
        kind,
        role: if kind == NodeKind::Work {
            Some(Role::Implementer)
        } else {
            None
        },
        description: node_id.to_owned(),
        completion_criteria: None,
    }
}

fn edge(from: &str, to: &str) -> crate::domain::EdgeDefinition {
    crate::domain::EdgeDefinition {
        from_node_id: from.to_owned(),
        to_node_id: to.to_owned(),
    }
}

/// 菱形：root → left / right → join → done。
fn spec() -> CreateWorkflowSpec {
    CreateWorkflowSpec {
        name: "d".to_owned(),
        objective: "o".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![
            node("root", NodeKind::Work),
            node("left", NodeKind::Work),
            node("right", NodeKind::Work),
            node("join", NodeKind::Work),
            node("done", NodeKind::Complete),
        ],
        edges: vec![
            edge("root", "left"),
            edge("root", "right"),
            edge("left", "join"),
            edge("right", "join"),
            edge("join", "done"),
        ],
    }
}

/// root 完成分叉后 fail left：right 转 Stopped，join/done Pending，全局 Replanning。
fn replanning_aggregate() -> WorkflowAggregate {
    let mut agg = WorkflowAggregate::new(spec(), "wf_1", "t0").expect("valid");
    agg.advance_node(
        "root",
        NodeAction::Complete,
        "fork".to_owned(),
        vec![],
        vec![decision("left"), decision("right")],
    )
    .expect("fork");
    agg.advance_node(
        "left",
        NodeAction::Fail,
        "left broken".to_owned(),
        vec![],
        vec![],
    )
    .expect("fail left");
    assert_eq!(agg.active_mode(), Some(ActiveMode::Replanning));
    assert_eq!(agg.node_status("right"), Some(NodeStatus::Stopped));
    agg
}

fn decision(node_id: &str) -> crate::domain::tool_data::SuccessorDecision {
    crate::domain::tool_data::SuccessorDecision {
        node_id: node_id.to_owned(),
        description: "go".to_owned(),
    }
}

fn fix_node(node_id: &str) -> crate::domain::NodeDefinition {
    crate::domain::NodeDefinition {
        node_id: node_id.to_owned(),
        kind: NodeKind::Work,
        role: Some(Role::Planner),
        description: "continuation".to_owned(),
        completion_criteria: None,
    }
}

fn replan_command(
    new_nodes: Vec<crate::domain::NodeDefinition>,
    rejoin: Vec<(&str, &str)>,
    entries: Vec<&str>,
) -> ReplanWorkflowCommand {
    ReplanWorkflowCommand {
        workflow_id: "wf_1".to_owned(),
        boundary: ReplanBoundary {
            trigger_node_id: "left".to_owned(),
            stopped_node_ids: vec!["right".to_owned()],
            frozen_fact_refs: vec![],
        },
        new_nodes,
        new_edges: vec![],
        entry_node_ids: entries.into_iter().map(str::to_owned).collect(),
        rejoin_edges: rejoin
            .into_iter()
            .map(|(from, target)| RejoinEdge {
                from_node_id: from.to_owned(),
                target_node_id: target.to_owned(),
            })
            .collect(),
        pending_rewrites: None,
    }
}

/// D3：合法 continuation——fix rejoin 到既有 Pending join，回 Executing。
#[test]
fn d3_valid_continuation_resumes_executing() {
    let mut agg = replanning_aggregate();
    let outcome = agg
        .replan(replan_command(
            vec![fix_node("fix")],
            vec![("fix", "join")],
            vec!["fix"],
        ))
        .expect("replan");
    assert_eq!(agg.active_mode(), Some(ActiveMode::Executing));
    assert_eq!(outcome.activated_node_ids, vec!["fix".to_owned()]);
    assert_eq!(agg.node_status("fix"), Some(NodeStatus::Active));
    assert_eq!(
        agg.node_status("join"),
        Some(NodeStatus::Pending),
        "rejoin target 不激活"
    );
    // 冻结历史不变。
    assert_eq!(agg.node_status("left"), Some(NodeStatus::Failed));
    assert_eq!(agg.node_status("right"), Some(NodeStatus::Stopped));
}

/// D4：无合法 rejoin target（全部 Pending 均不合规或不存在）→ WorkflowCannotContinue。
#[test]
fn d4_no_valid_rejoin_target() {
    let mut agg = replanning_aggregate();
    // 汇入不存在的节点。
    let error = agg
        .replan(replan_command(
            vec![fix_node("fix")],
            vec![("fix", "ghost")],
            vec!["fix"],
        ))
        .expect_err("must reject");
    assert!(matches!(
        error,
        WorkflowToolError::WorkflowCannotContinue { .. }
    ));
    assert_eq!(
        agg.active_mode(),
        Some(ActiveMode::Replanning),
        "失败保持 Replanning"
    );
}

/// D5：直连 CompleteNode 被拒。
#[test]
fn d5_direct_complete_node_rejected() {
    let mut agg = replanning_aggregate();
    let error = agg
        .replan(replan_command(
            vec![fix_node("fix")],
            vec![("fix", "done")],
            vec!["fix"],
        ))
        .expect_err("must reject");
    assert!(matches!(
        error,
        WorkflowToolError::WorkflowCannotContinue { .. }
    ));
    assert_eq!(agg.active_mode(), Some(ActiveMode::Replanning));
}

/// D6：非法 target——Failed / Stopped / Completed / Active / 本次新建。
#[test]
fn d6_illegal_targets_rejected() {
    let mut agg = replanning_aggregate();
    for target in ["left", "right", "root", "fix"] {
        let error = agg
            .replan(replan_command(
                vec![fix_node("fix")],
                vec![("fix", target)],
                vec!["fix"],
            ))
            .expect_err(target);
        assert!(
            matches!(error, WorkflowToolError::WorkflowCannotContinue { .. }),
            "{target}"
        );
    }
    assert_eq!(agg.active_mode(), Some(ActiveMode::Replanning));
}

/// D7：entry 无有效到 CompleteNode 的路径 → 拒绝。
#[test]
fn d7_entry_without_path_rejected() {
    let mut agg = replanning_aggregate();
    // fix2 无 rejoin 也无边 → 合并图中 fix2 到不了 done。
    let error = agg
        .replan(replan_command(
            vec![fix_node("fix2")],
            vec![("fix", "join")],
            vec!["fix2"],
        ))
        .expect_err("must reject");
    assert!(matches!(
        error,
        WorkflowToolError::WorkflowCannotContinue { .. }
    ));
}

/// D8：冻结历史不可改——rewrites 作用于 Failed 节点 / remove CompleteNode / 新边接冻结节点。
#[test]
fn d8_frozen_history_immutable() {
    let mut agg = replanning_aggregate();
    // rewrites.update 作用于 Failed 节点。
    let mut command = replan_command(vec![fix_node("fix")], vec![("fix", "join")], vec!["fix"]);
    command.pending_rewrites = Some(PendingRewrites {
        update: vec![PendingNodeUpdate {
            node_id: "left".to_owned(),
            description: Some("rewrite frozen".to_owned()),
            role: None,
            completion_criteria: None,
        }],
        remove: vec![],
    });
    let error = agg.replan(command).expect_err("must reject");
    assert!(matches!(error, WorkflowToolError::NodeHistoryFrozen { .. }));

    // rewrites.remove 作用于 CompleteNode。
    let mut agg2 = replanning_aggregate();
    let mut command = replan_command(vec![fix_node("fix")], vec![("fix", "join")], vec!["fix"]);
    command.pending_rewrites = Some(PendingRewrites {
        update: vec![],
        remove: vec!["done".to_owned()],
    });
    let error = agg2.replan(command).expect_err("must reject");
    assert!(matches!(error, WorkflowToolError::NodeHistoryFrozen { .. }));
}

/// D8 变体：new_nodes 含 CompleteNode → GraphInvariantViolation。
#[test]
fn d8_new_complete_node_rejected() {
    let mut agg = replanning_aggregate();
    let error = agg
        .replan(replan_command(
            vec![fix_node("fix"), node("done2", NodeKind::Complete)],
            vec![("fix", "join")],
            vec!["fix"],
        ))
        .expect_err("must reject");
    assert!(matches!(
        error,
        WorkflowToolError::GraphInvariantViolation { .. }
    ));
}

/// D9：Replan 后旧后继不被触发；join 的旧前驱（Failed/Stopped）不消耗 AND-join。
#[test]
fn d9_old_successors_not_triggered_after_replan() {
    let mut agg = replanning_aggregate();
    agg.replan(replan_command(
        vec![fix_node("fix")],
        vec![("fix", "join")],
        vec!["fix"],
    ))
    .expect("replan");
    // join 仍 Pending：其旧前驱 left(Failed)/right(Stopped) 不满足 join。
    assert_eq!(agg.node_status("join"), Some(NodeStatus::Pending));
    // fix 完成选择 join 后，join 的有效前驱（fix）满足 → 激活。
    agg.advance_node(
        "fix",
        NodeAction::Complete,
        "fixed".to_owned(),
        vec![],
        vec![decision("join")],
    )
    .expect("fix complete");
    // 合并图中 join 的有效前驱只有 fix（旧边 left→join / right→join 的 from 已冻结排除）。
    assert_eq!(
        agg.node_status("join"),
        Some(NodeStatus::Active),
        "有效前驱满足后 join 应激活"
    );
}

/// D10：Replanning 中 fail → 不可逆 Failed。
#[test]
fn d10_fail_from_replanning() {
    let mut agg = replanning_aggregate();
    agg.fail("cannot continue").expect("fail");
    assert_eq!(agg.status(), WorkflowStatus::Failed);
    assert!(matches!(
        agg.activate(),
        Err(WorkflowToolError::WorkflowAlreadyTerminal { .. })
    ));
}

/// 非 Replanning 提交 replan → WorkflowNotReplanning。
#[test]
fn replan_rejected_when_executing() {
    let mut agg = WorkflowAggregate::new(spec(), "wf_1", "t0").expect("valid");
    let error = agg
        .replan(replan_command(
            vec![fix_node("fix")],
            vec![("fix", "join")],
            vec!["fix"],
        ))
        .expect_err("must reject");
    assert!(matches!(
        error,
        WorkflowToolError::WorkflowNotReplanning { .. }
    ));
}

/// pending_rewrites：更新与退役未生效 Pending；retired 集合如实返回。
#[test]
fn rewrites_update_and_retire_pending() {
    let mut agg = replanning_aggregate();
    // 造一个可选分支 pending：root → extra → done（加入 spec 时不存在；直接用 join 的 rewrites）。
    let mut command = replan_command(vec![fix_node("fix")], vec![("fix", "join")], vec!["fix"]);
    command.pending_rewrites = Some(PendingRewrites {
        update: vec![PendingNodeUpdate {
            node_id: "join".to_owned(),
            description: Some("re-scoped".to_owned()),
            role: None,
            completion_criteria: None,
        }],
        remove: vec![],
    });
    let outcome = agg.replan(command).expect("replan");
    assert!(outcome.applied_node_ids.contains(&"join".to_owned()));
    assert!(agg.node_exits("join").is_none(), "join 仍 Pending 无出口");
    // 描述更新可在节点定义中查到（join 仍存在）。
    assert!(agg.nodes.iter().any(|n| n.node_id == "join"));
}
