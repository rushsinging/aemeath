use std::sync::Arc;

use context::adapters::decode_session;
use context::domain::session::{CanonicalSession, SessionCodec, SnapshotState};
use context::ports::SessionManagementPort;
use share::session_types::{PersistedWorkspaceContext, ProjectIdentity, WorkspaceId, WorktreeKind};

fn session_for_project(
    id: &str,
    identity: ProjectIdentity,
    workspace_root: &str,
) -> CanonicalSession {
    let mut session = CanonicalSession::fixture(id);
    session.workspace = SnapshotState::Captured(PersistedWorkspaceContext {
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
async fn session_management_filters_and_loads_only_matching_project_identity() {
    let root = std::env::temp_dir().join(format!(
        "aemeath-session-project-contract-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&root).expect("create storage root");
    let port: Arc<dyn SessionManagementPort> = Arc::new(
        context::adapters::AtomicBlobSessionManagement::new(Arc::new(
            storage::FileSystemBlobAdapter::new(&root).expect("create filesystem blob adapter"),
        )),
    );
    let project_a = ProjectIdentity {
        initial_cwd: "/project-a".to_string(),
        git_common_dir: Some("/project-a/.git".to_string()),
    };
    let project_b = ProjectIdentity {
        initial_cwd: "/project-b".to_string(),
        git_common_dir: Some("/project-b/.git".to_string()),
    };
    let same_git_other_worktree = ProjectIdentity {
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
            ProjectIdentity {
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
    assert!(matches!(
        port.load_for_project("project-b", &same_git_other_worktree).await,
        Err(context::SessionManagementError::ProjectMismatch(id)) if id == "project-b"
    ));
    assert!(matches!(
        port.export_for_project("project-b", &same_git_other_worktree)
            .await,
        Err(context::SessionManagementError::ProjectMismatch(id)) if id == "project-b"
    ));
    assert!(matches!(
        port.update_metadata_for_project(
            "project-b",
            &same_git_other_worktree,
            context::SessionMetadataUpdate {
                title: Some("forbidden".to_string()),
                ..Default::default()
            },
        )
        .await,
        Err(context::SessionManagementError::ProjectMismatch(id)) if id == "project-b"
    ));
    assert!(matches!(
        port.delete_for_project("project-b", &same_git_other_worktree)
            .await,
        Err(context::SessionManagementError::ProjectMismatch(id)) if id == "project-b"
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
    let blob = Arc::new(
        storage::FileSystemBlobAdapter::new(&root).expect("create filesystem blob adapter"),
    );
    let port: Arc<dyn SessionManagementPort> =
        Arc::new(context::adapters::AtomicBlobSessionManagement::new(blob));
    let project = ProjectIdentity {
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
    let port: Arc<dyn SessionManagementPort> = Arc::new(
        context::adapters::AtomicBlobSessionManagement::new(Arc::new(
            storage::FileSystemBlobAdapter::new(&root).expect("create filesystem blob adapter"),
        )),
    );
    let project = ProjectIdentity {
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
            context::SessionMetadataUpdate {
                title: Some("renamed".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("update session metadata");
    assert_eq!(updated.title.as_deref(), Some("renamed"));

    port.delete_for_project("session-lifecycle", &project)
        .await
        .expect("delete session");
    assert!(matches!(
        port.load_for_project("session-lifecycle", &project).await,
        Err(context::SessionManagementError::NotFound(_))
    ));
    std::fs::remove_dir_all(root).expect("remove storage root");
}
