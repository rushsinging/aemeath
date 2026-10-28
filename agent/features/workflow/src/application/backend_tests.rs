//! backend 端到端：B 组经完整端口往返（#2086）。

use std::sync::Arc;

use super::*;
use crate::domain::tool_data::{
    CreateWorkflowSpec, NodeAction, SuccessorDecision, UpdateNodeCommand,
};
use crate::domain::tool_error::WorkflowToolError;
use crate::domain::{NodeKind, NodeStatus, Role};
use crate::ports::WorkflowToolBackend;

fn backend() -> InMemoryWorkflowBackend {
    InMemoryWorkflowBackend::new(
        Arc::new(InMemoryWorkflowStore::new()),
        Box::new(|| "wf_1".to_owned()),
        Box::new(|| "t".to_owned()),
    )
}

fn diamond_spec() -> CreateWorkflowSpec {
    CreateWorkflowSpec {
        name: "d".to_owned(),
        objective: "o".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![
            crate::domain::NodeDefinition {
                node_id: "root".to_owned(),
                kind: NodeKind::Work,
                role: Some(Role::Planner),
                description: "r".to_owned(),
                completion_criteria: None,
            },
            crate::domain::NodeDefinition {
                node_id: "left".to_owned(),
                kind: NodeKind::Work,
                role: Some(Role::Implementer),
                description: "l".to_owned(),
                completion_criteria: None,
            },
            crate::domain::NodeDefinition {
                node_id: "join".to_owned(),
                kind: NodeKind::Work,
                role: Some(Role::Verifier),
                description: "j".to_owned(),
                completion_criteria: None,
            },
            crate::domain::NodeDefinition {
                node_id: "done".to_owned(),
                kind: NodeKind::Complete,
                role: None,
                description: "f".to_owned(),
                completion_criteria: None,
            },
        ],
        edges: vec![
            crate::domain::EdgeDefinition {
                from_node_id: "root".to_owned(),
                to_node_id: "left".to_owned(),
            },
            crate::domain::EdgeDefinition {
                from_node_id: "root".to_owned(),
                to_node_id: "join".to_owned(),
            },
            crate::domain::EdgeDefinition {
                from_node_id: "left".to_owned(),
                to_node_id: "join".to_owned(),
            },
            crate::domain::EdgeDefinition {
                from_node_id: "join".to_owned(),
                to_node_id: "done".to_owned(),
            },
        ],
    }
}

fn node_update(
    workflow_id: &str,
    node_id: &str,
    action: NodeAction,
    successors: Vec<&str>,
) -> UpdateNodeCommand {
    UpdateNodeCommand {
        workflow_id: workflow_id.to_owned(),
        node_id: node_id.to_owned(),
        action,
        conclusion: "c".to_owned(),
        evidence_refs: Vec::new(),
        successors: successors
            .into_iter()
            .map(|node_id| SuccessorDecision {
                node_id: node_id.to_owned(),
                description: "d".to_owned(),
            })
            .collect(),
    }
}

/// B1 端到端：root complete 单后继 → activated 与查询一致。
#[tokio::test]
async fn b1_backend_round_trip_single_successor() {
    let backend = backend();
    backend.create(diamond_spec()).await.expect("create");
    let outcome = backend
        .node_update(node_update(
            "wf_1",
            "root",
            NodeAction::Complete,
            vec!["left"],
        ))
        .await
        .expect("root complete");
    assert_eq!(outcome.node_status, NodeStatus::Completed);
    assert_eq!(outcome.activated_node_ids, vec!["left".to_owned()]);

    let detail = backend
        .query(crate::domain::tool_data::QueryWorkflowQuery {
            workflow_id: "wf_1".to_owned(),
            include_facts: false,
        })
        .await
        .expect("query");
    let root = detail.nodes.iter().find(|n| n.node_id == "root").unwrap();
    assert_eq!(root.status, NodeStatus::Completed);
    assert_eq!(root.successors.len(), 1);
    assert_eq!(root.successors[0].node_id, "left");
    let left = detail.nodes.iter().find(|n| n.node_id == "left").unwrap();
    assert_eq!(left.status, NodeStatus::Active);
}

/// B4 端到端：双前驱 AND-join——部分完成 join 仍 Pending。
#[tokio::test]
async fn b4_backend_and_join_activates_once_when_all_predecessors_done() {
    let backend = backend();
    backend.create(diamond_spec()).await.expect("create");
    backend
        .node_update(node_update(
            "wf_1",
            "root",
            NodeAction::Complete,
            vec!["left", "join"],
        ))
        .await
        .expect("fork");
    // root 完成时 join 的另一前驱 left 未完成 → root 那次不激活 join；
    // left 完成后两个前驱（root、left）均 Completed 且均选择 join → 激活一次。
    let outcome = backend
        .node_update(node_update(
            "wf_1",
            "left",
            NodeAction::Complete,
            vec!["join"],
        ))
        .await
        .expect("left complete");
    assert_eq!(
        outcome.activated_node_ids,
        vec!["join".to_owned()],
        "join 应在全部前驱完成后恰好激活一次"
    );
    let detail = backend
        .query(crate::domain::tool_data::QueryWorkflowQuery {
            workflow_id: "wf_1".to_owned(),
            include_facts: false,
        })
        .await
        .expect("query");
    let join = detail.nodes.iter().find(|n| n.node_id == "join").unwrap();
    assert_eq!(join.status, NodeStatus::Active);
}

/// B2 端到端：缺 successors → InvalidSuccessor 且状态不变。
#[tokio::test]
async fn b2_backend_missing_successors_rejected() {
    let backend = backend();
    backend.create(diamond_spec()).await.expect("create");
    let error = backend
        .node_update(node_update("wf_1", "root", NodeAction::Complete, vec![]))
        .await
        .expect_err("must reject");
    assert!(matches!(error, WorkflowToolError::InvalidSuccessor { .. }));
    let detail = backend
        .query(crate::domain::tool_data::QueryWorkflowQuery {
            workflow_id: "wf_1".to_owned(),
            include_facts: false,
        })
        .await
        .expect("query");
    let root = detail.nodes.iter().find(|n| n.node_id == "root").unwrap();
    assert_eq!(root.status, NodeStatus::Active, "状态不变");
}
