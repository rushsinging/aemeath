use share::ids::SessionId;
use std::sync::Arc;

use crate::decode_session;
use crate::domain::session::SessionSnapshotState;
use crate::SessionManagementPort;
use crate::ToolCallIdentityData;
use crate::{CanonicalSession, SessionCodec};
use share::session_types::{
    PersistedWorkspaceContext, ProjectIdentityData, WorkspaceId, WorktreeKind,
};

fn session_for_project(
    id: &str,
    identity: ProjectIdentityData,
    workspace_root: &str,
) -> CanonicalSession {
    let mut session = CanonicalSession::fixture(id);
    session.workspace = SessionSnapshotState::Captured(PersistedWorkspaceContext {
        workspace_id: WorkspaceId::derive(&identity, workspace_root),
        project_identity: identity,
        path_base: workspace_root.to_string(),
        workspace_root: workspace_root.to_string(),
        worktree_kind: WorktreeKind::Primary,
        context_stack: Vec::new(),
    });
    session
}

#[tokio::test]
async fn session_management_migrates_legacy_tool_receipt_ledger_on_resume() {
    let root = std::env::temp_dir().join(format!(
        "aemeath-tool-receipt-ledger-contract-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&root).expect("create storage root");
    let blob: Arc<dyn storage::AtomicBlobPort> =
        storage::wire_file_system_blob(&root).expect("create filesystem blob adapter");
    let port = crate::adapters::AtomicBlobSessionManagement::new(Arc::clone(&blob));
    // C 阶段：sidecar 退役，本测试验证一次性迁移读——用旧 ledger 直写构造存量数据。
    let project = ProjectIdentityData {
        initial_cwd: "/receipt-ledger".to_string(),
        git_common_dir: None,
    };
    let session_id = SessionId::new("receipt-ledger");
    let session = session_for_project(session_id.as_str(), project.clone(), "/receipt-ledger");
    port.import_for_project(
        &SessionCodec::encode(&session).expect("encode session"),
        &project,
    )
    .await
    .expect("persist session");
    let identity = ToolCallIdentityData {
        session_id: session_id.clone(),
        run_id: sdk::RunId::new("run"),
        step_id: share::ids::RunStepId::new("step"),
        runtime_call_id: "call-1".to_string(),
        provider_call_id: Some("provider-1".to_string()),
        tool_name: "Glob".to_string(),
        call_index: 0,
        agent: false,
    };
    let receipt = crate::tool_receipt::ToolCallReceiptData::pending(identity, "safe preview");
    crate::adapters::tool_receipt_ledger::AtomicBlobToolReceiptLedger::new(
        blob,
        session_id.as_str(),
    )
    .expect("open legacy ledger")
    .save(1, &receipt)
    .await
    .expect("persist receipt ledger");

    let resumed = port
        .load_for_project(session_id.as_str(), &project)
        .await
        .expect("resume session with receipt ledger");

    assert_eq!(resumed.revision, 0);
    assert_eq!(resumed.run_slices[0].steps[0].tool_receipts[0], receipt);
    std::fs::remove_dir_all(root).expect("remove storage root");
}

#[tokio::test]
async fn session_management_filters_and_loads_only_matching_project_identity() {
    let root = std::env::temp_dir().join(format!(
        "aemeath-session-project-contract-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&root).expect("create storage root");
    let port: Arc<dyn SessionManagementPort> =
        Arc::new(crate::adapters::AtomicBlobSessionManagement::new(
            storage::wire_file_system_blob(&root).expect("create filesystem blob adapter"),
        ));
    let project_a = ProjectIdentityData {
        initial_cwd: "/project-a".to_string(),
        git_common_dir: Some("/project-a/.git".to_string()),
    };
    let project_b = ProjectIdentityData {
        initial_cwd: "/project-b".to_string(),
        git_common_dir: Some("/project-b/.git".to_string()),
    };
    let same_git_other_worktree = ProjectIdentityData {
        initial_cwd: "/project-a/.worktrees/feature".to_string(),
        git_common_dir: Some("/project-a/.git".to_string()),
    };

    for (session, project) in [
        (
            session_for_project("project-a", project_a.clone(), "/project-a"),
            same_git_other_worktree.clone(),
        ),
        (
            session_for_project("project-b", project_b, "/project-b"),
            ProjectIdentityData {
                initial_cwd: "/project-b".to_string(),
                git_common_dir: Some("/project-b/.git".to_string()),
            },
        ),
    ] {
        port.import_for_project(
            &SessionCodec::encode(&session).expect("encode session"),
            &project,
        )
        .await
        .expect("persist session");
    }

    let visible = port
        .list_for_project(&same_git_other_worktree)
        .await
        .expect("list matching project sessions");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, "project-a");
    assert!(port
        .load_for_project("project-a", &same_git_other_worktree)
        .await
        .is_ok());
    // 按 project 分目录布局：跨项目 session 在本项目视图下不可见（探测
    // 序列只含本项目目录段与平铺遗留位置），NotFound 与旧 ProjectMismatch
    // 在消费方同归 SessionResumeFailureKind::NotFound，语义等价且更准确。
    assert!(matches!(
        port.load_for_project("project-b", &same_git_other_worktree).await,
        Err(crate::SessionManagementError::NotFound(id)) if id == "project-b"
    ));
    assert!(matches!(
        port.export_for_project("project-b", &same_git_other_worktree)
            .await,
        Err(crate::SessionManagementError::NotFound(id)) if id == "project-b"
    ));
    assert!(matches!(
        port.update_metadata_for_project(
            "project-b",
            &same_git_other_worktree,
            // 覆盖式更新：载荷为整份 metadata（此处仅构造最小形态）。
            crate::session::SessionMetadata {
                title: Some("forbidden".to_string()),
                ..Default::default()
            },
        )
        .await,
        Err(crate::SessionManagementError::NotFound(id)) if id == "project-b"
    ));
    assert!(matches!(
        port.delete_for_project("project-b", &same_git_other_worktree)
            .await,
        Err(crate::SessionManagementError::NotFound(id)) if id == "project-b"
    ));
    let exported = port
        .export_for_project("project-a", &same_git_other_worktree)
        .await
        .expect("same Git project may export a session");
    assert_eq!(
        decode_session(&exported)
            .expect("decode exported session")
            .session
            .id,
        "project-a"
    );
    drop(port);
    std::fs::remove_dir_all(root).expect("remove storage root");
}

#[tokio::test]
async fn session_management_lists_only_primary_sessions_for_current_project() {
    let root = std::env::temp_dir().join(format!(
        "aemeath-session-management-contract-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&root).expect("create storage root");
    let blob = storage::wire_file_system_blob(&root).expect("create filesystem blob adapter");
    let port: Arc<dyn SessionManagementPort> =
        Arc::new(crate::adapters::AtomicBlobSessionManagement::new(blob));
    let project = ProjectIdentityData {
        initial_cwd: "/session-primary".to_string(),
        git_common_dir: None,
    };
    let session = session_for_project("session-primary", project.clone(), "/session-primary");

    port.import_for_project(
        &SessionCodec::encode(&session).expect("encode session"),
        &project,
    )
    .await
    .expect("persist primary session");

    let sessions = port
        .list_for_project(&project)
        .await
        .expect("list current project sessions");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "session-primary");
    std::fs::remove_dir_all(root).expect("remove storage root");
}

#[tokio::test]
async fn session_management_imports_exports_updates_and_deletes_through_injected_blob() {
    let root = std::env::temp_dir().join(format!(
        "aemeath-session-management-contract-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&root).expect("create storage root");
    let port: Arc<dyn SessionManagementPort> =
        Arc::new(crate::adapters::AtomicBlobSessionManagement::new(
            storage::wire_file_system_blob(&root).expect("create filesystem blob adapter"),
        ));
    let project = ProjectIdentityData {
        initial_cwd: "/session-lifecycle".to_string(),
        git_common_dir: None,
    };
    let session = session_for_project("session-lifecycle", project.clone(), "/session-lifecycle");

    let imported = port
        .import_for_project(
            &SessionCodec::encode(&session).expect("encode session"),
            &project,
        )
        .await
        .expect("import session");
    assert_eq!(imported.id, "session-lifecycle");

    let exported = port
        .export_for_project("session-lifecycle", &project)
        .await
        .expect("export session");
    assert_eq!(
        decode_session(&exported)
            .expect("decode exported session")
            .session
            .id,
        "session-lifecycle"
    );

    let updated = port
        .update_metadata_for_project(
            "session-lifecycle",
            &project,
            crate::session::SessionMetadata {
                title: Some("renamed".to_string()),
                model: Some("Zhipu Coding Plan/glm-5.3".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("update session metadata");
    assert_eq!(updated.title.as_deref(), Some("renamed"));
    assert_eq!(updated.model.as_deref(), Some("Zhipu Coding Plan/glm-5.3"));
    let reloaded = port
        .load_for_project("session-lifecycle", &project)
        .await
        .expect("reload session after metadata update");
    assert_eq!(
        reloaded.metadata.model.as_deref(),
        Some("Zhipu Coding Plan/glm-5.3")
    );

    port.delete_for_project("session-lifecycle", &project)
        .await
        .expect("delete session");
    assert!(matches!(
        port.load_for_project("session-lifecycle", &project).await,
        Err(crate::SessionManagementError::NotFound(_))
    ));
    std::fs::remove_dir_all(root).expect("remove storage root");
}
