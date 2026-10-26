//! Kernel 八条结构不变量的 A1–A7 验收测试（设计文档 §11.2 A 组）。
//!
//! TDD 红阶段：`Kernel::validate` 尚未实现，全部用例必须失败（panic 于
//! unimplemented）。实现完成后转绿，且失败用例必须断言具体 reason。

use super::*;

fn work_node(node_id: &str) -> NodeDefinition {
    NodeDefinition {
        node_id: node_id.to_owned(),
        kind: NodeKind::Work,
        role: Some(Role::Implementer),
        description: format!("{node_id} 工作"),
        completion_criteria: None,
    }
}

fn special_node(node_id: &str, kind: NodeKind) -> NodeDefinition {
    NodeDefinition {
        node_id: node_id.to_owned(),
        kind,
        role: None,
        description: if kind == NodeKind::Complete {
            "完成定义".to_owned()
        } else {
            "需确认事项".to_owned()
        },
        completion_criteria: None,
    }
}

fn edge(from: &str, to: &str) -> EdgeDefinition {
    EdgeDefinition {
        from_node_id: from.to_owned(),
        to_node_id: to.to_owned(),
    }
}

/// 最小合法提案：root WorkNode → CompleteNode。
fn minimal_definition() -> WorkflowDefinition {
    WorkflowDefinition {
        name: "mvp".to_owned(),
        objective: "交付".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![work_node("root"), special_node("done", NodeKind::Complete)],
        edges: vec![edge("root", "done")],
    }
}

/// A1：最小合法 Graph 通过校验。
#[test]
fn a1_minimal_valid_graph_passes() {
    assert_eq!(Kernel::validate(&minimal_definition()), Ok(()));
}

/// A1 变体：菱形（root 分叉 + 汇聚）与 ConfirmNode 在合法位置均通过。
#[test]
fn a1_diamond_with_confirm_passes() {
    let definition = WorkflowDefinition {
        name: "diamond".to_owned(),
        objective: "并行与确认".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![
            work_node("root"),
            work_node("left"),
            work_node("right"),
            special_node("gate", NodeKind::Confirm),
            special_node("done", NodeKind::Complete),
        ],
        edges: vec![
            edge("root", "left"),
            edge("root", "right"),
            edge("left", "gate"),
            edge("right", "gate"),
            edge("gate", "done"),
        ],
    };
    assert_eq!(Kernel::validate(&definition), Ok(()));
}

/// A2：无 CompleteNode。
#[test]
fn a2_missing_complete_node_rejected() {
    let mut definition = minimal_definition();
    definition
        .nodes
        .retain(|node| node.kind != NodeKind::Complete);
    definition.edges.clear();
    assert_eq!(
        Kernel::validate(&definition),
        Err(GraphError {
            reason: GraphInvariantReason::MissingCompleteNode,
            subject: None
        })
    );
}

/// A3：多个 root（两个无前驱 Node）。
#[test]
fn a3_multiple_roots_rejected() {
    let mut definition = minimal_definition();
    definition.nodes.push(work_node("orphan"));
    definition.edges.push(edge("orphan", "done"));
    assert_eq!(
        Kernel::validate(&definition),
        Err(GraphError {
            reason: GraphInvariantReason::MultipleRoots,
            subject: None
        })
    );
}

/// A4：有环（root → a → b → a）。
#[test]
fn a4_cycle_rejected() {
    let cyclic = WorkflowDefinition {
        name: "cycle".to_owned(),
        objective: "环".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![
            work_node("root"),
            work_node("a"),
            work_node("b"),
            special_node("done", NodeKind::Complete),
        ],
        edges: vec![
            edge("root", "a"),
            edge("a", "b"),
            edge("b", "a"),
            edge("a", "done"),
        ],
    };
    assert_eq!(
        Kernel::validate(&cyclic),
        Err(GraphError {
            reason: GraphInvariantReason::CycleDetected,
            subject: None
        })
    );
}

/// A4 变体：自环。
#[test]
fn a4_self_loop_rejected() {
    let mut definition = minimal_definition();
    definition.nodes.push(work_node("loop"));
    definition.edges.push(edge("root", "loop"));
    definition.edges.push(edge("loop", "loop"));
    definition.edges.push(edge("loop", "done"));
    assert_eq!(
        Kernel::validate(&definition),
        Err(GraphError {
            reason: GraphInvariantReason::SelfLoop,
            subject: Some("loop".to_owned())
        })
    );
}

/// A5：非 CompleteNode 无后继。
#[test]
fn a5_node_without_successor_rejected() {
    let mut definition = minimal_definition();
    definition.nodes.push(work_node("dead"));
    definition.edges.push(edge("root", "dead"));
    assert_eq!(
        Kernel::validate(&definition),
        Err(GraphError {
            reason: GraphInvariantReason::NodeWithoutSuccessor,
            subject: Some("dead".to_owned())
        })
    );
}

/// A7：CompleteNode 有后继。
#[test]
fn a7_complete_node_with_successor_rejected() {
    let mut definition = minimal_definition();
    definition.nodes.push(work_node("after"));
    definition.edges.push(edge("done", "after"));
    assert_eq!(
        Kernel::validate(&definition),
        Err(GraphError {
            reason: GraphInvariantReason::CompleteNodeHasSuccessor,
            subject: Some("done".to_owned())
        })
    );
}

/// Kernel 3：root 必须是普通业务 Node。
#[test]
fn root_must_be_work_kind() {
    let definition = WorkflowDefinition {
        name: "bad-root".to_owned(),
        objective: "root 是 ConfirmNode".to_owned(),
        root_node_id: "gate".to_owned(),
        nodes: vec![
            special_node("gate", NodeKind::Confirm),
            special_node("done", NodeKind::Complete),
        ],
        edges: vec![edge("gate", "done")],
    };
    assert_eq!(
        Kernel::validate(&definition),
        Err(GraphError {
            reason: GraphInvariantReason::RootIsSpecialKind,
            subject: Some("gate".to_owned())
        })
    );
}

/// Kernel 5：存在无法到达 CompleteNode 的 Node。
#[test]
fn unreachable_to_complete_rejected() {
    let dead_end = WorkflowDefinition {
        name: "dead-branch".to_owned(),
        objective: "断头".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![
            work_node("root"),
            work_node("mid"),
            work_node("tail"),
            special_node("done", NodeKind::Complete),
        ],
        edges: vec![
            edge("root", "done"),
            edge("root", "mid"),
            edge("mid", "tail"),
        ],
    };
    // tail 无后继（A5）与 mid 无 complete 路径两条违规；Kernel 顺序决定先报哪个，
    // 断言接受两者之一，由实现固定优先级后收紧。
    let result = Kernel::validate(&dead_end);
    assert!(matches!(
        result,
        Err(GraphError {
            reason: GraphInvariantReason::NoPathToCompleteNode
                | GraphInvariantReason::NodeWithoutSuccessor,
            ..
        })
    ));
}

/// Kernel 7：Edge 端点必须引用已定义 Node。
#[test]
fn dangling_edge_rejected() {
    let mut definition = minimal_definition();
    definition.edges.push(edge("root", "ghost"));
    assert_eq!(
        Kernel::validate(&definition),
        Err(GraphError {
            reason: GraphInvariantReason::MissingNode,
            subject: Some("ghost".to_owned())
        })
    );
}

/// Kernel 10：Role 约束——work 必须有 Role，confirm/complete 禁止 Role。
#[test]
fn role_constraints_enforced() {
    let mut no_role = minimal_definition();
    no_role.nodes[0].role = None;
    assert_eq!(
        Kernel::validate(&no_role),
        Err(GraphError {
            reason: GraphInvariantReason::WorkNodeMissingRole,
            subject: Some("root".to_owned())
        })
    );

    let mut role_on_complete = minimal_definition();
    role_on_complete.nodes[1].role = Some(Role::Reviewer);
    assert_eq!(
        Kernel::validate(&role_on_complete),
        Err(GraphError {
            reason: GraphInvariantReason::RoleOnSpecialKind,
            subject: Some("done".to_owned())
        })
    );
}

/// Kernel 4：从 root 不可达的 Node。
#[test]
fn unreachable_from_root_rejected() {
    let definition = WorkflowDefinition {
        name: "unreachable".to_owned(),
        objective: "孤岛".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![
            work_node("root"),
            work_node("island"),
            special_node("done", NodeKind::Complete),
        ],
        edges: vec![edge("root", "done"), edge("island", "done")],
    };
    // island 无前驱（第二个 root）且从 root 不可达；两种违规任一即拒。
    let result = Kernel::validate(&definition);
    assert!(matches!(
        result,
        Err(GraphError {
            reason: GraphInvariantReason::MultipleRoots | GraphInvariantReason::UnreachableNode,
            ..
        })
    ));
}
