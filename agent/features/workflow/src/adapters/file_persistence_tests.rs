//! 文件系统持久化测试（#2083；F 组 + 损坏 + 原子性）。

use std::fs;
use std::sync::Arc;

use super::*;
use crate::application::InMemoryWorkflowStore;
use crate::domain::snapshot::WorkflowSnapshotData;
use crate::domain::tool_data::CreateWorkflowSpec;
use crate::domain::{NodeKind, Role};
use crate::ports::{PersistenceError, WorkflowPersistence, WorkflowToolBackend};

fn snapshot(workflow_id: &str) -> WorkflowSnapshotData {
    let spec = CreateWorkflowSpec {
        name: "n".to_owned(),
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
    };
    crate::application::WorkflowAggregate::new(spec, workflow_id, "t")
        .expect("valid")
        .to_snapshot()
}

#[test]
fn save_then_load_all_round_trip() {
    let dir = tempfile::tempdir().expect("dir");
    let persistence = FileWorkflowPersistence::new(dir.path());
    persistence
        .save("session_1", &snapshot("wf_a"))
        .expect("save");
    let loaded = persistence.load_all("session_1").expect("load");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].workflow_id, "wf_a");
    assert_eq!(loaded[0].node_entries.len(), 2);
}

#[test]
fn clear_index_keeps_entities() {
    let dir = tempfile::tempdir().expect("dir");
    let persistence = FileWorkflowPersistence::new(dir.path());
    persistence
        .save("session_1", &snapshot("wf_a"))
        .expect("save");
    persistence.clear_index("session_1").expect("clear");
    // F4：实体仍在磁盘。
    assert!(dir.path().join("entities/wf_a.json").exists());
    // F3：索引清空后 load_all 为空。
    let loaded = persistence.load_all("session_1").expect("load");
    assert!(loaded.is_empty());
}

#[test]
fn corrupt_entity_fails_closed() {
    let dir = tempfile::tempdir().expect("dir");
    let persistence = FileWorkflowPersistence::new(dir.path());
    persistence
        .save("session_1", &snapshot("wf_a"))
        .expect("save");
    // 破坏实体文件。
    fs::write(dir.path().join("entities/wf_a.json"), b"{ broken").expect("write");
    let error = persistence.load_all("session_1").expect_err("must fail");
    assert!(matches!(error, PersistenceError::Corrupt(_)), "{error:?}");
}

#[test]
fn missing_entity_referenced_by_index_fails_closed() {
    let dir = tempfile::tempdir().expect("dir");
    let persistence = FileWorkflowPersistence::new(dir.path());
    persistence
        .save("session_1", &snapshot("wf_a"))
        .expect("save");
    fs::remove_file(dir.path().join("entities/wf_a.json")).expect("remove");
    let error = persistence.load_all("session_1").expect_err("must fail");
    assert!(matches!(error, PersistenceError::Corrupt(_)), "{error:?}");
}

#[test]
fn atomic_write_leaves_no_temp_residue() {
    let dir = tempfile::tempdir().expect("dir");
    let persistence = FileWorkflowPersistence::new(dir.path());
    for round in 0..3 {
        let mut snap = snapshot("wf_a");
        snap.updated_at = format!("t{round}");
        persistence.save("session_1", &snap).expect("save");
    }
    let entities: Vec<_> = fs::read_dir(dir.path().join("entities"))
        .expect("entities dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(entities, vec!["wf_a.json".to_string()], "无 .tmp 残留");
    let index: Vec<_> = fs::read_dir(dir.path().join("index"))
        .expect("index dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert_eq!(index, vec!["session_1.json".to_string()]);
}

/// 端到端：文件持久层 + backend 写穿 + resume（F5 三态经真实文件）。
#[tokio::test]
async fn end_to_end_resume_via_files() {
    let dir = tempfile::tempdir().expect("dir");
    let persistence = Arc::new(FileWorkflowPersistence::new(dir.path()));

    let spec = CreateWorkflowSpec {
        name: "e2e".to_owned(),
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
    };

    // 第一段：创建后挂起（Replanning → deactivate → Inactive(pending=Replanning)）。
    {
        let backend = crate::application::InMemoryWorkflowBackend::new(
            Arc::new(InMemoryWorkflowStore::with_persistence(
                persistence.clone(),
                "session_e2e",
            )),
            Box::new(|| "wf_e2e".to_owned()),
            Box::new(|| "t".to_owned()),
        );
        backend.create(spec.clone()).await.expect("create");
        backend
            .node_update(crate::domain::tool_data::UpdateNodeCommand {
                workflow_id: "wf_e2e".to_owned(),
                node_id: "root".to_owned(),
                action: crate::domain::tool_data::NodeAction::Fail,
                conclusion: "boom".to_owned(),
                evidence_refs: vec![],
                successors: vec![],
            })
            .await
            .expect("fail");
        backend
            .update(crate::domain::tool_data::UpdateWorkflowCommand {
                workflow_id: "wf_e2e".to_owned(),
                action: crate::domain::tool_data::LifecycleAction::Deactivate,
                summary: None,
                evidence_refs: vec![],
            })
            .await
            .expect("deactivate");
    }

    // 第二段：resume——恢复 Inactive(pending_mode=Replanning)（F5 第三态）。
    let resumed = crate::application::InMemoryWorkflowBackend::new(
        Arc::new(InMemoryWorkflowStore::with_persistence(
            persistence.clone(),
            "session_e2e",
        )),
        Box::new(|| "wf_x".to_owned()),
        Box::new(|| "t".to_owned()),
    );
    let detail = resumed
        .query(crate::domain::tool_data::QueryWorkflowQuery {
            workflow_id: "wf_e2e".to_owned(),
            include_facts: false,
        })
        .await
        .expect("query");
    assert_eq!(detail.status, crate::domain::WorkflowStatus::Inactive);
    assert_eq!(
        detail.pending_mode,
        Some(crate::domain::ActiveMode::Replanning)
    );
    assert_eq!(detail.nodes[0].status, crate::domain::NodeStatus::Failed);
}
