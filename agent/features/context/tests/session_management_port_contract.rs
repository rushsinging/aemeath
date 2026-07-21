use std::sync::Arc;

use context::adapters::decode_session;
use context::domain::session::{CanonicalSession, SessionCodec};
use context::ports::SessionManagementPort;

#[tokio::test]
async fn session_management_lists_only_primary_canonical_sessions() {
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
    let session = CanonicalSession::fixture("session-primary");

    port.import(&SessionCodec::encode(&session).expect("encode session"))
        .await
        .expect("persist primary session");

    let sessions = port.list().await.expect("list primary sessions");
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
    let session = CanonicalSession::fixture("session-lifecycle");

    let imported = port
        .import(&SessionCodec::encode(&session).expect("encode session"))
        .await
        .expect("import session");
    assert_eq!(imported.id, "session-lifecycle");

    let exported = port
        .export("session-lifecycle")
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
        .update_metadata(
            "session-lifecycle",
            context::SessionMetadataUpdate {
                title: Some("renamed".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("update session metadata");
    assert_eq!(updated.title.as_deref(), Some("renamed"));

    port.delete("session-lifecycle")
        .await
        .expect("delete session");
    assert!(matches!(
        port.load_canonical("session-lifecycle").await,
        Err(context::SessionManagementError::NotFound(_))
    ));
    std::fs::remove_dir_all(root).expect("remove storage root");
}
