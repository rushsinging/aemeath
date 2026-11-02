//! 持久化快照往返与 write-through 测试（设计文档 §11.2 F3–F5；#2083 红阶段）。

use std::sync::{Arc, Mutex};

use super::*;
use crate::domain::tool_data::{CreateWorkflowSpec, NodeAction};
use crate::domain::{ActiveMode, NodeKind, NodeStatus, Role, WorkflowStatus};
use crate::ports::{PersistenceError, WorkflowPersistence, WorkflowToolBackend};

fn spec() -> CreateWorkflowSpec {
    CreateWorkflowSpec {
        name: "p".to_owned(),
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
                node_id: "done".to_owned(),
                kind: NodeKind::Complete,
                role: None,
                description: "d".to_owned(),
                completion_criteria: None,
            },
        ],
        edges: vec![crate::domain::EdgeDefinition {
            from_node_id: "root".to_owned(),
            to_node_id: "done".to_owned(),
        }],
    }
}

fn aggregate(mode: &str) -> WorkflowAggregate {
    let mut agg = WorkflowAggregate::new(spec(), "wf_1", "t0").expect("valid");
    match mode {
        "replanning" => agg.enter_replanning(),
        "inactive" => {
            agg.enter_replanning();
            agg.deactivate().expect("deactivate");
        }
        _ => {}
    }
    agg
}

/// 快照往返：三种状态逐一恢复一致（F5）。
#[test]
fn snapshot_round_trip_three_modes() {
    for mode in ["executing", "replanning", "inactive"] {
        let agg = aggregate(mode);
        let snapshot = agg.to_snapshot();
        let restored = WorkflowAggregate::from_snapshot(snapshot).expect("restore");
        assert_eq!(restored.status(), agg.status(), "{mode}");
        assert_eq!(restored.active_mode(), agg.active_mode(), "{mode}");
        assert_eq!(restored.pending_mode(), agg.pending_mode(), "{mode}");
        assert_eq!(
            restored.node_status("root"),
            agg.node_status("root"),
            "{mode}"
        );
        assert_eq!(restored.name, agg.name, "{mode}");
        assert_eq!(restored.edges.len(), agg.edges.len(), "{mode}");
    }
}

/// 快照含结论 / 证据 / 出口决定 / 冻结历史。
#[test]
fn snapshot_carries_full_node_state() {
    let mut agg = aggregate("executing");
    agg.advance_node(
        "root",
        NodeAction::Complete,
        "done it".to_owned(),
        vec![crate::domain::tool_data::EvidenceRef {
            fact_id: "f1".to_owned(),
            kind: crate::domain::tool_data::EvidenceKind::ToolResult,
        }],
        vec![crate::domain::tool_data::SuccessorDecision {
            node_id: "done".to_owned(),
            description: "finish".to_owned(),
        }],
    )
    .expect("root complete");
    let snapshot = agg.to_snapshot();
    let text = serde_json::to_string(&snapshot).expect("serde");
    let restored = WorkflowAggregate::from_snapshot(serde_json::from_str(&text).expect("parse"))
        .expect("restore");
    assert_eq!(restored.node_conclusion("root"), Some("done it"));
    assert_eq!(restored.node_evidence("root").unwrap().len(), 1);
    assert_eq!(restored.node_exits("root").unwrap().len(), 1);
    assert_eq!(restored.node_status("root"), Some(NodeStatus::Completed));
}

/// 测试用内存持久层：捕获快照，可编程 load_all。
#[derive(Default)]
struct FakePersistence {
    session_id: Mutex<String>,
    saved: Mutex<Vec<crate::domain::snapshot::WorkflowSnapshotData>>,
    cleared: Mutex<Vec<String>>,
}

impl WorkflowPersistence for FakePersistence {
    fn save(
        &self,
        session_id: &str,
        snapshot: &crate::domain::snapshot::WorkflowSnapshotData,
    ) -> Result<(), PersistenceError> {
        *self.session_id.lock().unwrap() = session_id.to_owned();
        self.saved.lock().unwrap().push(snapshot.clone());
        Ok(())
    }

    fn load_all(
        &self,
        _session_id: &str,
    ) -> Result<Vec<crate::domain::snapshot::WorkflowSnapshotData>, PersistenceError> {
        // clear 后索引为空：实体仍在 saved，但 load_all 不再返回（F3 语义）。
        if !self.cleared.lock().unwrap().is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.saved.lock().unwrap().clone())
    }

    fn clear_index(&self, session_id: &str) -> Result<(), PersistenceError> {
        self.cleared.lock().unwrap().push(session_id.to_owned());
        Ok(())
    }
}

fn backend_with_persistence() -> (InMemoryWorkflowBackend, Arc<FakePersistence>) {
    let persistence = Arc::new(FakePersistence::default());
    let backend = InMemoryWorkflowBackend::new(
        Arc::new(InMemoryWorkflowStore::with_persistence(
            persistence.clone(),
            "session_1".to_owned(),
        )),
        Box::new(|| "wf_1".to_owned()),
        Box::new(|| "t".to_owned()),
    );
    (backend, persistence)
}

/// write-through：create 成功即落盘，重载后 list/query 可见。
#[tokio::test]
async fn create_persists_and_reload_restores() {
    let (backend, persistence) = backend_with_persistence();
    backend.create(spec()).await.expect("create");
    assert_eq!(persistence.saved.lock().unwrap().len(), 1, "create 即落盘");

    // 模拟 resume：新 store 从持久层加载。
    let reloaded = InMemoryWorkflowBackend::new(
        Arc::new(InMemoryWorkflowStore::with_persistence(
            persistence.clone(),
            "session_1".to_owned(),
        )),
        Box::new(|| "wf_x".to_owned()),
        Box::new(|| "t".to_owned()),
    );
    let list = reloaded
        .list(crate::domain::tool_data::ListWorkflowsQuery {
            status: None,
            limit: 20,
            offset: 0,
        })
        .await
        .expect("list");
    assert_eq!(list.total, 1, "resume 后可见");
    let detail = reloaded
        .query(crate::domain::tool_data::QueryWorkflowQuery {
            workflow_id: "wf_1".to_owned(),
            include_facts: false,
        })
        .await
        .expect("query");
    assert_eq!(detail.status, WorkflowStatus::Active);
    assert_eq!(detail.active_mode, Some(ActiveMode::Executing));
}

/// F3 语义：clear_index 后重载 → 不可见（WorkflowNotVisible）。
#[tokio::test]
async fn clear_index_makes_workflows_invisible() {
    let (backend, persistence) = backend_with_persistence();
    backend.create(spec()).await.expect("create");
    persistence.clear_index("session_1").expect("clear");

    let reloaded = InMemoryWorkflowBackend::new(
        Arc::new(InMemoryWorkflowStore::with_persistence(
            persistence.clone(),
            "session_1".to_owned(),
        )),
        Box::new(|| "wf_x".to_owned()),
        Box::new(|| "t".to_owned()),
    );
    let list = reloaded
        .list(crate::domain::tool_data::ListWorkflowsQuery {
            status: None,
            limit: 20,
            offset: 0,
        })
        .await
        .expect("list");
    assert_eq!(list.total, 0, "F3：clear 后 list 为空");
    let error = reloaded
        .query(crate::domain::tool_data::QueryWorkflowQuery {
            workflow_id: "wf_1".to_owned(),
            include_facts: false,
        })
        .await
        .expect_err("F3：按 id 直接访问被拒");
    assert!(matches!(
        error,
        crate::domain::tool_error::WorkflowToolError::WorkflowNotVisible { .. }
    ));
}

/// update / node_update / replan 成功后均 write-through。
#[tokio::test]
async fn mutations_persist_snapshot() {
    let (backend, persistence) = backend_with_persistence();
    backend.create(spec()).await.expect("create");
    let before = persistence.saved.lock().unwrap().len();
    backend
        .update(crate::domain::tool_data::UpdateWorkflowCommand {
            workflow_id: "wf_1".to_owned(),
            action: crate::domain::tool_data::LifecycleAction::Deactivate,
            summary: None,
            evidence_refs: vec![],
        })
        .await
        .expect("deactivate");
    assert_eq!(
        persistence.saved.lock().unwrap().len(),
        before + 1,
        "update 落盘"
    );

    // Inactive(pending_mode=None) 快照重载仍 Inactive。
    let last = persistence.saved.lock().unwrap().last().unwrap().clone();
    assert_eq!(last.status, WorkflowStatus::Inactive);
}
