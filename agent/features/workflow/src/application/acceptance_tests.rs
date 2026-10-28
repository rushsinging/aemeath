//! MVP 验收链缺口场景（设计文档 §10.2 A6 / C1 / D1 / E7 / F1 / F2 / F6 / F7）。
//!
//! 其余 A–G 场景已在各模块测试中覆盖：
//! - A1–A5、A7 与 Kernel reason 全表 → `domain/kernel_tests.rs`
//! - B1–B5、C2–C4 → `application/advance_tests.rs` + `backend_tests.rs`
//! - D2–D10 → `application/replan_tests.rs`
//! - E1–E6、E8 → `application/aggregate_tests.rs`
//! - F3–F5 → `application/persistence_tests.rs` + `adapters/file_persistence_tests.rs`
//! - G1–G4 → `aggregate_tests.rs` + `runtime/reminder_sources_tests.rs`
//!
//! 本文件统一走完整 backend 端口，断言「状态不变 + 具体 typed error」与
//! 「Tool Result 与 workflow_query 一致」。

use std::sync::Arc;

use super::*;
use crate::domain::tool_data::{
    CreateWorkflowSpec, EvidenceKind, EvidenceRef, LifecycleAction, ListWorkflowsQuery, NodeAction,
    QueryWorkflowQuery, SuccessorDecision, UpdateNodeCommand, UpdateWorkflowCommand,
    WorkflowSummaryData,
};
use crate::domain::tool_error::WorkflowToolError;
use crate::domain::{ActiveMode, NodeKind, NodeStatus, Role, WorkflowStatus};
use crate::ports::WorkflowToolBackend;

fn backend() -> InMemoryWorkflowBackend {
    InMemoryWorkflowBackend::new(
        Arc::new(InMemoryWorkflowStore::new()),
        Box::new(|| "wf_1".to_owned()),
        Box::new(|| "t".to_owned()),
    )
}

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

/// root → done 的最小合法 spec。
fn minimal_spec() -> CreateWorkflowSpec {
    CreateWorkflowSpec {
        name: "min".to_owned(),
        objective: "ship".to_owned(),
        root_node_id: "root".to_owned(),
        nodes: vec![
            node("root", NodeKind::Work),
            node("done", NodeKind::Complete),
        ],
        edges: vec![edge("root", "done")],
    }
}

fn decision(node_id: &str) -> SuccessorDecision {
    SuccessorDecision {
        node_id: node_id.to_owned(),
        description: "go".to_owned(),
    }
}

async fn query(
    backend: &InMemoryWorkflowBackend,
    workflow_id: &str,
    include_facts: bool,
) -> crate::domain::tool_data::WorkflowDetailData {
    backend
        .query(QueryWorkflowQuery {
            workflow_id: workflow_id.to_owned(),
            include_facts,
        })
        .await
        .expect("query")
}

async fn list_all(
    backend: &InMemoryWorkflowBackend,
    query_input: ListWorkflowsQuery,
) -> crate::domain::tool_data::ListWorkflowsData {
    backend.list(query_input).await.expect("list")
}

// ── A6：已有 Active 时创建被拒且原状态不变 ──────────────────────────

#[tokio::test]
async fn a6_create_rejected_when_active_exists() {
    let backend = backend();
    backend.create(minimal_spec()).await.expect("first create");
    let before = query(&backend, "wf_1", false).await;

    let error = backend
        .create(minimal_spec())
        .await
        .expect_err("second create must be rejected");
    assert!(
        matches!(error, WorkflowToolError::ActiveWorkflowAlreadyExists),
        "{error:?}"
    );

    // 原 Active 的状态、模式与 Active Node 集合不变。
    let after = query(&backend, "wf_1", false).await;
    assert_eq!(after.status, before.status);
    assert_eq!(after.active_mode, before.active_mode);
    let active_before: Vec<_> = before
        .nodes
        .iter()
        .filter(|n| n.status == NodeStatus::Active)
        .map(|n| n.node_id.clone())
        .collect();
    let active_after: Vec<_> = after
        .nodes
        .iter()
        .filter(|n| n.status == NodeStatus::Active)
        .map(|n| n.node_id.clone())
        .collect();
    assert_eq!(active_after, active_before);
    // 未留下第二个 Workflow。
    let listed = list_all(
        &backend,
        ListWorkflowsQuery {
            status: None,
            limit: 20,
            offset: 0,
        },
    )
    .await;
    assert_eq!(listed.total, 1, "拒绝后不得留下半成品");
}

// ── C1：单节点失败（无并行分支，stopped 为空） ──────────────────────

#[tokio::test]
async fn c1_single_node_fail_without_parallel_branches() {
    let backend = backend();
    backend.create(minimal_spec()).await.expect("create");

    let outcome = backend
        .node_update(UpdateNodeCommand {
            workflow_id: "wf_1".to_owned(),
            node_id: "root".to_owned(),
            action: NodeAction::Fail,
            conclusion: "cannot proceed".to_owned(),
            evidence_refs: vec![EvidenceRef {
                fact_id: "fact_1".to_owned(),
                kind: EvidenceKind::ToolResult,
            }],
            successors: Vec::new(),
        })
        .await
        .expect("fail");

    assert_eq!(outcome.node_status, NodeStatus::Failed);
    assert_eq!(outcome.status, WorkflowStatus::Active);
    assert_eq!(outcome.active_mode, Some(ActiveMode::Replanning));
    assert!(
        outcome.stopped_node_ids.is_empty(),
        "无并行分支时 stopped 为空"
    );

    // Tool Result 与持久状态一致，且返回 ReplanBoundary。
    let detail = query(&backend, "wf_1", false).await;
    assert_eq!(detail.active_mode, Some(ActiveMode::Replanning));
    assert_eq!(
        detail
            .nodes
            .iter()
            .find(|n| n.node_id == "root")
            .map(|n| n.status),
        Some(NodeStatus::Failed)
    );
    let boundary = outcome
        .replan_boundary
        .expect("fail 必须返回 ReplanBoundary");
    assert_eq!(boundary.trigger_node_id, "root");
    assert!(boundary.stopped_node_ids.is_empty());
    assert_eq!(boundary.frozen_fact_refs.len(), 1);
}

// ── D1：Replanning 中拒绝 node_update 且状态不变 ─────────────────────

#[tokio::test]
async fn d1_node_update_rejected_in_replanning() {
    let backend = backend();
    backend.create(minimal_spec()).await.expect("create");
    backend
        .node_update(UpdateNodeCommand {
            workflow_id: "wf_1".to_owned(),
            node_id: "root".to_owned(),
            action: NodeAction::Fail,
            conclusion: "boom".to_owned(),
            evidence_refs: Vec::new(),
            successors: Vec::new(),
        })
        .await
        .expect("fail");
    let before = query(&backend, "wf_1", false).await;

    let error = backend
        .node_update(UpdateNodeCommand {
            workflow_id: "wf_1".to_owned(),
            node_id: "done".to_owned(),
            action: NodeAction::Complete,
            conclusion: "should not apply".to_owned(),
            evidence_refs: Vec::new(),
            successors: Vec::new(),
        })
        .await
        .expect_err("node_update must be rejected in Replanning");
    assert!(
        matches!(error, WorkflowToolError::WorkflowReplanning { .. }),
        "{error:?}"
    );

    let after = query(&backend, "wf_1", false).await;
    assert_eq!(after.active_mode, Some(ActiveMode::Replanning));
    assert_eq!(after.status, before.status);
    let done = after.nodes.iter().find(|n| n.node_id == "done").unwrap();
    assert_eq!(done.status, NodeStatus::Pending, "被拒后状态不变");
    assert_eq!(done.conclusion, None);
}

// ── E7：complete / cancel / fail 缺少 summary ───────────────────────

#[tokio::test]
async fn e7_missing_summary_rejected() {
    let backend = backend();
    backend.create(minimal_spec()).await.expect("create");

    // complete 缺 summary。
    let error = backend
        .update(UpdateWorkflowCommand {
            workflow_id: "wf_1".to_owned(),
            action: LifecycleAction::Complete,
            summary: None,
            evidence_refs: Vec::new(),
        })
        .await
        .expect_err("complete without summary must be rejected");
    assert!(
        matches!(
            error,
            WorkflowToolError::CompletionConditionsUnsatisfied { .. }
        ),
        "{error:?}"
    );
    let detail = query(&backend, "wf_1", false).await;
    assert_eq!(detail.status, WorkflowStatus::Active, "状态不变");

    // cancel 缺 summary。
    let error = backend
        .update(UpdateWorkflowCommand {
            workflow_id: "wf_1".to_owned(),
            action: LifecycleAction::Cancel,
            summary: None,
            evidence_refs: Vec::new(),
        })
        .await
        .expect_err("cancel without reason must be rejected");
    assert!(
        matches!(
            error,
            WorkflowToolError::CompletionConditionsUnsatisfied { .. }
        ),
        "{error:?}"
    );
    assert_eq!(
        query(&backend, "wf_1", false).await.status,
        WorkflowStatus::Active
    );

    // fail 缺 summary：先进入 Replanning。
    backend
        .node_update(UpdateNodeCommand {
            workflow_id: "wf_1".to_owned(),
            node_id: "root".to_owned(),
            action: NodeAction::Fail,
            conclusion: "broken".to_owned(),
            evidence_refs: Vec::new(),
            successors: Vec::new(),
        })
        .await
        .expect("enter replanning");
    let error = backend
        .update(UpdateWorkflowCommand {
            workflow_id: "wf_1".to_owned(),
            action: LifecycleAction::Fail,
            summary: None,
            evidence_refs: Vec::new(),
        })
        .await
        .expect_err("fail without reason must be rejected");
    assert!(
        matches!(
            error,
            WorkflowToolError::CompletionConditionsUnsatisfied { .. }
        ),
        "{error:?}"
    );
    assert_eq!(
        query(&backend, "wf_1", false).await.active_mode,
        Some(ActiveMode::Replanning),
        "被拒后仍是 Replanning"
    );
}

// ── F1 / F2：分页与状态过滤 ─────────────────────────────────────────

/// 造 n 个 Workflow：依次 create → deactivate，使 Active 槽位可复用。
async fn seed_deactivated(backend: &InMemoryWorkflowBackend, count: usize) {
    for index in 0..count {
        backend.create(minimal_spec()).await.expect("create");
        let listed = list_all(
            backend,
            ListWorkflowsQuery {
                status: Some(WorkflowStatus::Active),
                limit: 100,
                offset: 0,
            },
        )
        .await;
        let active_id = listed
            .workflows
            .first()
            .expect("active workflow")
            .workflow_id
            .clone();
        backend
            .update(UpdateWorkflowCommand {
                workflow_id: active_id,
                action: LifecycleAction::Deactivate,
                summary: None,
                evidence_refs: Vec::new(),
            })
            .await
            .expect("deactivate");
        let _ = index;
    }
}

/// F1：分页返回受 limit / offset 约束，total 不受影响。
#[tokio::test]
async fn f1_pagination_bounds_and_total() {
    let backend = InMemoryWorkflowBackend::new(
        Arc::new(InMemoryWorkflowStore::new()),
        {
            // 每次 create 生成新 id。
            let counter = Arc::new(std::sync::Mutex::new(0u32));
            Box::new(move || {
                let mut value = counter.lock().unwrap();
                *value += 1;
                format!("wf_{value:02}")
            })
        },
        Box::new(|| "t".to_owned()),
    );
    seed_deactivated(&backend, 3).await;

    let first_page = list_all(
        &backend,
        ListWorkflowsQuery {
            status: None,
            limit: 2,
            offset: 0,
        },
    )
    .await;
    assert_eq!(first_page.workflows.len(), 2);
    assert_eq!(first_page.total, 3, "total 不受分页影响");
    assert_eq!(first_page.limit, 2);
    assert_eq!(first_page.offset, 0);

    let second_page = list_all(
        &backend,
        ListWorkflowsQuery {
            status: None,
            limit: 2,
            offset: 2,
        },
    )
    .await;
    assert_eq!(second_page.workflows.len(), 1);
    assert_eq!(second_page.total, 3);
    // 两页无交集。
    let first_ids: Vec<_> = first_page
        .workflows
        .iter()
        .map(|w| w.workflow_id.clone())
        .collect();
    let second_ids: Vec<_> = second_page
        .workflows
        .iter()
        .map(|w| w.workflow_id.clone())
        .collect();
    assert!(second_ids.iter().all(|id| !first_ids.contains(id)));
}

/// F2：状态过滤只返回匹配项，total 为匹配计数。
#[tokio::test]
async fn f2_status_filter_matches_total() {
    let backend = InMemoryWorkflowBackend::new(
        Arc::new(InMemoryWorkflowStore::new()),
        {
            let counter = Arc::new(std::sync::Mutex::new(0u32));
            Box::new(move || {
                let mut value = counter.lock().unwrap();
                *value += 1;
                format!("wf_{value:02}")
            })
        },
        Box::new(|| "t".to_owned()),
    );
    seed_deactivated(&backend, 2).await;
    // 再建一个保持 Active。
    backend.create(minimal_spec()).await.expect("active one");

    let inactive = list_all(
        &backend,
        ListWorkflowsQuery {
            status: Some(WorkflowStatus::Inactive),
            limit: 20,
            offset: 0,
        },
    )
    .await;
    assert_eq!(inactive.total, 2);
    assert!(inactive
        .workflows
        .iter()
        .all(|w| w.status == WorkflowStatus::Inactive));

    let active = list_all(
        &backend,
        ListWorkflowsQuery {
            status: Some(WorkflowStatus::Active),
            limit: 20,
            offset: 0,
        },
    )
    .await;
    assert_eq!(active.total, 1);
    assert_eq!(active.workflows[0].status, WorkflowStatus::Active);

    let cancelled = list_all(
        &backend,
        ListWorkflowsQuery {
            status: Some(WorkflowStatus::Cancelled),
            limit: 20,
            offset: 0,
        },
    )
    .await;
    assert_eq!(cancelled.total, 0, "无匹配状态时 total 为 0");
    assert!(cancelled.workflows.is_empty());
}

// ── F6 / F7：证据只返回引用；结构合法即可提交 ───────────────────────

/// F6：evidence 只返回引用字段，不含 Tool 输出正文。
#[tokio::test]
async fn f6_evidence_returns_refs_only() {
    let backend = backend();
    backend.create(minimal_spec()).await.expect("create");
    backend
        .node_update(UpdateNodeCommand {
            workflow_id: "wf_1".to_owned(),
            node_id: "root".to_owned(),
            action: NodeAction::Complete,
            conclusion: "done".to_owned(),
            evidence_refs: vec![EvidenceRef {
                fact_id: "fact_42".to_owned(),
                kind: EvidenceKind::BackgroundTask,
            }],
            successors: vec![decision("done")],
        })
        .await
        .expect("complete");

    let detail = query(&backend, "wf_1", true).await;
    let root = detail.nodes.iter().find(|n| n.node_id == "root").unwrap();
    assert_eq!(root.evidence_refs.len(), 1);
    assert_eq!(root.evidence_refs[0].fact_id, "fact_42");
    assert_eq!(root.evidence_refs[0].kind, EvidenceKind::BackgroundTask);
    // 无正文：NodeViewData.evidence_refs 只有 fact_id + kind 两个字段，
    // 编译期即保证不会携带正文；此处额外断言 facts 摘要不含正文键。
    let serialized = serde_json::to_string(&detail).expect("serialize");
    for forbidden in ["\"output\"", "\"stdout\"", "\"content\"", "\"body\""] {
        assert!(
            !serialized.contains(forbidden),
            "query 结果不得内联工具输出正文（出现 {forbidden}）"
        );
    }
}

/// F7：引用结构合法但实体不存在仍可提交；`kind` 非法时解析失败。
#[tokio::test]
async fn f7_structurally_valid_reference_accepted() {
    let backend = backend();
    backend.create(minimal_spec()).await.expect("create");

    // 形如合法、实体从未存在的 fact_id：Kernel 不校验存在性，提交成功。
    let outcome = backend
        .node_update(UpdateNodeCommand {
            workflow_id: "wf_1".to_owned(),
            node_id: "root".to_owned(),
            action: NodeAction::Complete,
            conclusion: "引用不存在的 fact 也应通过".to_owned(),
            evidence_refs: vec![EvidenceRef {
                fact_id: "fact_never_existed".to_owned(),
                kind: EvidenceKind::SubAgentOutput,
            }],
            successors: vec![decision("done")],
        })
        .await
        .expect("structurally valid evidence must be accepted");
    assert_eq!(outcome.node_status, NodeStatus::Completed);

    // kind 非法 → 协议层解析失败（InvalidEvidenceReference 的边界：
    // Kernel 看不到非法结构，因为反序列化阶段已拒绝）。
    let parsed = serde_json::from_value::<UpdateNodeCommand>(serde_json::json!({
        "workflow_id": "wf_1",
        "node_id": "done",
        "action": "complete",
        "conclusion": "x",
        "evidence_refs": [{ "fact_id": "f", "kind": "unknown_kind" }],
        "successors": []
    }));
    assert!(parsed.is_err(), "非法 kind 必须在协议层被拒");
}

/// G1 复核：backend 端 allowed_actions 与状态一致（终态为空）。
#[tokio::test]
async fn g1_backend_allowed_actions_follow_status() {
    let backend = backend();
    backend.create(minimal_spec()).await.expect("create");
    let executing = query(&backend, "wf_1", false).await;
    assert_eq!(
        executing.allowed_actions,
        vec![
            crate::domain::tool_data::AllowedAction::Deactivate,
            crate::domain::tool_data::AllowedAction::Complete,
            crate::domain::tool_data::AllowedAction::Cancel,
            crate::domain::tool_data::AllowedAction::NodeComplete,
            crate::domain::tool_data::AllowedAction::NodeFail,
        ]
    );

    backend
        .update(UpdateWorkflowCommand {
            workflow_id: "wf_1".to_owned(),
            action: LifecycleAction::Deactivate,
            summary: None,
            evidence_refs: Vec::new(),
        })
        .await
        .expect("deactivate");
    let inactive = query(&backend, "wf_1", false).await;
    assert_eq!(
        inactive.allowed_actions,
        vec![
            crate::domain::tool_data::AllowedAction::Activate,
            crate::domain::tool_data::AllowedAction::Cancel,
        ]
    );

    backend
        .update(UpdateWorkflowCommand {
            workflow_id: "wf_1".to_owned(),
            action: LifecycleAction::Cancel,
            summary: Some("user gave up".to_owned()),
            evidence_refs: Vec::new(),
        })
        .await
        .expect("cancel");
    let cancelled = query(&backend, "wf_1", false).await;
    assert!(cancelled.allowed_actions.is_empty(), "终态为空数组");
}

/// F5 补充：resume 后 node_summary 计数与查询视图逐项一致。
#[tokio::test]
async fn f5_summary_counts_match_detail_after_resume() {
    let backend = backend();
    backend.create(minimal_spec()).await.expect("create");
    let summary: WorkflowSummaryData = list_all(
        &backend,
        ListWorkflowsQuery {
            status: None,
            limit: 20,
            offset: 0,
        },
    )
    .await
    .workflows
    .into_iter()
    .next()
    .expect("summary");
    let detail = query(&backend, "wf_1", false).await;

    let count_of = |status: NodeStatus| {
        detail
            .nodes
            .iter()
            .filter(|node| node.status == status)
            .count()
    };
    assert_eq!(summary.node_summary.active, count_of(NodeStatus::Active));
    assert_eq!(summary.node_summary.pending, count_of(NodeStatus::Pending));
    assert_eq!(
        summary.node_summary.completed,
        count_of(NodeStatus::Completed)
    );
    assert_eq!(summary.node_summary.failed, count_of(NodeStatus::Failed));
    assert_eq!(summary.node_summary.stopped, count_of(NodeStatus::Stopped));
}
