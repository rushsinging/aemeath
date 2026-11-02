//! 节点推进与隐式 AND-join 测试（设计文档 §11.2 B1–B5、C3–C4；#2086 TDD 红阶段）。

use super::*;
use crate::domain::tool_data::{EvidenceKind, EvidenceRef, NodeAction, SuccessorDecision};
use crate::domain::tool_error::WorkflowToolError;
use crate::domain::{ActiveMode, NodeKind, NodeStatus, Role, WorkflowStatus};

fn node(node_id: &str, kind: NodeKind, role: Option<Role>) -> crate::domain::NodeDefinition {
    crate::domain::NodeDefinition {
        node_id: node_id.to_owned(),
        kind,
        role,
        description: format!("{node_id} work"),
        completion_criteria: None,
    }
}

fn work(node_id: &str) -> crate::domain::NodeDefinition {
    node(node_id, NodeKind::Work, Some(Role::Implementer))
}

fn edge(from: &str, to: &str) -> crate::domain::EdgeDefinition {
    crate::domain::EdgeDefinition {
        from_node_id: from.to_owned(),
        to_node_id: to.to_owned(),
    }
}

fn successor(node_id: &str) -> SuccessorDecision {
    SuccessorDecision {
        node_id: node_id.to_owned(),
        description: "next step".to_owned(),
    }
}

/// 菱形：root → left / right → join → done。
fn diamond() -> WorkflowAggregate {
    let spec = crate::domain::tool_data::CreateWorkflowSpec {
        name: "diamond".to_owned(),
        objective: "join".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![
            work("root"),
            work("left"),
            work("right"),
            work("join"),
            node("done", NodeKind::Complete, None),
        ],
        edges: vec![
            edge("root", "left"),
            edge("root", "right"),
            edge("left", "join"),
            edge("right", "join"),
            edge("join", "done"),
        ],
    };
    WorkflowAggregate::new(spec, "wf_1", "t0").expect("valid diamond")
}

/// B1：单后继完成 → 后继激活，activated 一致。
#[test]
fn b1_single_successor_activates_next() {
    let mut agg = diamond();
    let outcome = agg
        .advance_node(
            "root",
            NodeAction::Complete,
            "both ready".to_owned(),
            vec![],
            vec![successor("left")],
        )
        .expect("root complete");
    assert_eq!(outcome.node_status, NodeStatus::Completed);
    assert_eq!(outcome.activated_node_ids, vec!["left".to_owned()]);
    assert_eq!(agg.node_status("left"), Some(NodeStatus::Active));
    assert_eq!(
        agg.node_status("right"),
        Some(NodeStatus::Pending),
        "未选择的后继不激活"
    );
    assert_eq!(agg.status(), WorkflowStatus::Active);
}

/// B2：非 CompleteNode complete 缺 successors → InvalidSuccessor，状态不变。
#[test]
fn b2_missing_successors_rejected() {
    let mut agg = diamond();
    let before = agg.node_status("root");
    assert!(matches!(
        agg.advance_node(
            "root",
            NodeAction::Complete,
            "done".to_owned(),
            vec![],
            vec![]
        ),
        Err(WorkflowToolError::InvalidSuccessor { .. })
    ));
    assert_eq!(agg.node_status("root"), before, "节点状态不变");
    assert_eq!(agg.node_status("left"), Some(NodeStatus::Pending));
}

/// B3：successors 指向非直接后继 → InvalidSuccessor，节点保持 Active。
#[test]
fn b3_non_adjacent_successor_rejected() {
    let mut agg = diamond();
    assert!(matches!(
        agg.advance_node(
            "root",
            NodeAction::Complete,
            "skip".to_owned(),
            vec![],
            vec![successor("join")]
        ),
        Err(WorkflowToolError::InvalidSuccessor { .. })
    ));
    assert_eq!(agg.node_status("root"), Some(NodeStatus::Active));
    // join 不得被激活。
    assert_eq!(agg.node_status("join"), Some(NodeStatus::Pending));
}

/// B4：并行 + AND-join——部分完成保持 Pending，全齐且均选择后只激活一次。
#[test]
fn b4_and_join_activates_once() {
    let mut agg = diamond();
    agg.advance_node(
        "root",
        NodeAction::Complete,
        "fork".to_owned(),
        vec![],
        vec![successor("left"), successor("right")],
    )
    .expect("root complete forks");
    assert_eq!(agg.node_status("left"), Some(NodeStatus::Active));
    assert_eq!(agg.node_status("right"), Some(NodeStatus::Active));

    // left 完成 → join 仍 Pending（right 未完成）。
    agg.advance_node(
        "left",
        NodeAction::Complete,
        "left done".to_owned(),
        vec![],
        vec![successor("join")],
    )
    .expect("left complete");
    assert_eq!(agg.node_status("join"), Some(NodeStatus::Pending));

    // right 完成 → join 激活（一次）。
    let outcome = agg
        .advance_node(
            "right",
            NodeAction::Complete,
            "right done".to_owned(),
            vec![],
            vec![successor("join")],
        )
        .expect("right complete");
    assert_eq!(outcome.activated_node_ids, vec!["join".to_owned()]);
    assert_eq!(agg.node_status("join"), Some(NodeStatus::Active));
}

/// B5：CompleteNode 完成不提交 successors；Workflow 不自动 Completed。
#[test]
fn b5_complete_node_finish_without_successors() {
    let mut agg = diamond();
    agg.advance_node(
        "root",
        NodeAction::Complete,
        "fork".to_owned(),
        vec![],
        vec![successor("left"), successor("right")],
    )
    .expect("fork");
    agg.advance_node(
        "left",
        NodeAction::Complete,
        "l".to_owned(),
        vec![],
        vec![successor("join")],
    )
    .expect("left");
    agg.advance_node(
        "right",
        NodeAction::Complete,
        "r".to_owned(),
        vec![],
        vec![successor("join")],
    )
    .expect("right");
    agg.advance_node(
        "join",
        NodeAction::Complete,
        "joined".to_owned(),
        vec![],
        vec![successor("done")],
    )
    .expect("join complete");

    // done 是 CompleteNode：完成时不提交 successors（传了要拒）。
    assert!(matches!(
        agg.advance_node(
            "done",
            NodeAction::Complete,
            "delivered".to_owned(),
            vec![],
            vec![successor("done")]
        ),
        Err(WorkflowToolError::InvalidSuccessor { .. })
    ));
    let outcome = agg
        .advance_node(
            "done",
            NodeAction::Complete,
            "delivered".to_owned(),
            vec![],
            vec![],
        )
        .expect("complete node done");
    assert_eq!(outcome.node_status, NodeStatus::Completed);
    assert!(outcome.activated_node_ids.is_empty());
    assert_eq!(agg.status(), WorkflowStatus::Active, "不自动 Completed");
    assert_eq!(agg.active_mode(), Some(ActiveMode::Executing));
}

/// fail 携带 successors → InvalidSuccessor。
#[test]
fn fail_with_successors_rejected() {
    let mut agg = diamond();
    assert!(matches!(
        agg.advance_node(
            "root",
            NodeAction::Fail,
            "broken".to_owned(),
            vec![],
            vec![successor("left")]
        ),
        Err(WorkflowToolError::InvalidSuccessor { .. })
    ));
    assert_eq!(
        agg.node_status("root"),
        Some(NodeStatus::Active),
        "状态不变"
    );
}

/// 同一 successors 列表内重复 node_id → InvalidSuccessor。
#[test]
fn duplicate_successors_rejected() {
    let mut agg = diamond();
    assert!(matches!(
        agg.advance_node(
            "root",
            NodeAction::Complete,
            "dup".to_owned(),
            vec![],
            vec![successor("left"), successor("left")]
        ),
        Err(WorkflowToolError::InvalidSuccessor { .. })
    ));
}

/// C3：Stopped 节点不可更新。
#[test]
fn c3_stopped_node_rejected() {
    let mut agg = diamond();
    // 先 fork 出两个 Active，再 fail left → right 转 Stopped。
    agg.advance_node(
        "root",
        NodeAction::Complete,
        "fork".to_owned(),
        vec![],
        vec![successor("left"), successor("right")],
    )
    .expect("fork");
    agg.advance_node(
        "left",
        NodeAction::Fail,
        "left broken".to_owned(),
        vec![],
        vec![],
    )
    .expect("left fail");
    assert_eq!(agg.node_status("right"), Some(NodeStatus::Stopped));
    // fail 使全局进入 Replanning，node_update 被更外层 Replanning 不变量
    // 先拒（D1 优先）；Stopped → NodeNotActive 的独立断言在 #2087（Replan
    // 成功回 Executing 后对遗留 Stopped 节点提交）补测。
    assert!(matches!(
        agg.advance_node(
            "right",
            NodeAction::Complete,
            "late".to_owned(),
            vec![],
            vec![successor("join")]
        ),
        Err(WorkflowToolError::WorkflowReplanning { .. })
    ));
    assert_eq!(
        agg.node_status("right"),
        Some(NodeStatus::Stopped),
        "状态不变"
    );
}

/// C4：终态节点（Completed / Failed）不可更新。
#[test]
fn c4_terminal_nodes_rejected() {
    let mut agg = diamond();
    agg.advance_node(
        "root",
        NodeAction::Complete,
        "done".to_owned(),
        vec![],
        vec![successor("left")],
    )
    .expect("root complete");
    // root 已 Completed。
    assert!(matches!(
        agg.advance_node(
            "root",
            NodeAction::Complete,
            "again".to_owned(),
            vec![],
            vec![successor("left")]
        ),
        Err(WorkflowToolError::NodeHistoryFrozen { .. })
    ));
    // fail 节点为 Failed；fail 使全局进入 Replanning，node_update 被
    // 更外层的 Replanning 不变量先拒（D1 优先于节点级冻结）。
    agg.advance_node("left", NodeAction::Fail, "dead".to_owned(), vec![], vec![])
        .expect("left fail");
    assert_eq!(agg.node_status("left"), Some(NodeStatus::Failed));
    assert!(matches!(
        agg.advance_node(
            "left",
            NodeAction::Complete,
            "revive".to_owned(),
            vec![],
            vec![]
        ),
        Err(WorkflowToolError::WorkflowReplanning { .. })
    ));
}

/// 结论与证据记录；出口决定可通过查询视图读取。
#[test]
fn conclusion_evidence_and_exits_recorded() {
    let mut agg = diamond();
    let evidence = vec![EvidenceRef {
        fact_id: "fact_7".to_owned(),
        kind: EvidenceKind::BackgroundTask,
    }];
    agg.advance_node(
        "root",
        NodeAction::Complete,
        "plan ok".to_owned(),
        evidence.clone(),
        vec![successor("left")],
    )
    .expect("complete");
    assert_eq!(agg.node_conclusion("root"), Some("plan ok"));
    assert_eq!(agg.node_evidence("root"), Some(evidence));
    assert_eq!(agg.node_exits("root"), Some(vec![successor("left")]));
}

/// Failed / Stopped 前驱不满足 AND-join（部分前驱永不完成 → 汇聚点保持 Pending）。
#[test]
fn stopped_predecessor_does_not_satisfy_join() {
    let mut agg = diamond();
    agg.advance_node(
        "root",
        NodeAction::Complete,
        "fork".to_owned(),
        vec![],
        vec![successor("left"), successor("right")],
    )
    .expect("fork");
    // left fail → right Stopped（并行全停）。join 的两个前驱都不满足 → Pending。
    agg.advance_node("left", NodeAction::Fail, "boom".to_owned(), vec![], vec![])
        .expect("fail");
    assert_eq!(agg.node_status("join"), Some(NodeStatus::Pending));
    assert_eq!(agg.active_mode(), Some(ActiveMode::Replanning));
}
