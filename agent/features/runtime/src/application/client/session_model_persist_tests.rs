use super::*;
use share::session_types::ProjectIdentityData;
use std::sync::Arc;

/// #2146：实现权归 context crate。stored_model 决定 load 出的 metadata.model。
fn model_port(stored: Option<&str>) -> Arc<context::test_support::ScriptedSessionManagement> {
    let stored = stored.map(str::to_string);
    Arc::new(context::test_support::ScriptedSessionManagement::new(
        move |id| {
            let mut session = context::session::CanonicalSession::fixture(id);
            session.metadata.model = stored.clone();
            session
        },
    ))
}

/// updates 投影为 (id, model) 元组（与原断言口径一致）。
fn recorded_models(
    port: &context::test_support::ScriptedSessionManagement,
) -> Vec<(String, Option<String>)> {
    port.updates()
        .into_iter()
        .map(|m| ("session-1".to_string(), m.model))
        .collect()
}

#[test]
fn session_model_key_joins_source_and_model_id() {
    assert_eq!(
        session_model_key("Zhipu Coding Plan", "glm-5.3"),
        "Zhipu Coding Plan/glm-5.3"
    );
}

#[tokio::test]
async fn persist_session_model_writes_metadata_model() {
    let port = model_port(None);
    let project = ProjectIdentityData {
        initial_cwd: "/tmp/proj".into(),
        git_common_dir: None,
    };
    persist_session_model(&*port, "session-1", &project, "Zhipu Coding Plan/glm-5.3").await;
    let updates = recorded_models(&port);
    assert_eq!(
        updates,
        vec![(
            "session-1".to_string(),
            Some("Zhipu Coding Plan/glm-5.3".to_string())
        )]
    );
}

#[tokio::test]
async fn persist_session_model_skips_blank_model() {
    let port = model_port(None);
    let project = ProjectIdentityData {
        initial_cwd: "/tmp/proj".into(),
        git_common_dir: None,
    };
    persist_session_model(&*port, "session-1", &project, "  ").await;
    assert!(port.updates().is_empty());
}

#[tokio::test]
async fn persist_session_model_skips_when_metadata_model_unchanged() {
    let port = model_port(Some("OmniRoute/cursor-auto"));
    let project = ProjectIdentityData {
        initial_cwd: "/tmp/proj".into(),
        git_common_dir: None,
    };
    persist_session_model(&*port, "session-1", &project, "OmniRoute/cursor-auto").await;
    assert!(
        port.updates().is_empty(),
        "same model must not bump session generation"
    );
}

#[tokio::test]
async fn persist_session_model_writes_when_metadata_model_differs() {
    let port = model_port(Some("OmniRoute/old"));
    let project = ProjectIdentityData {
        initial_cwd: "/tmp/proj".into(),
        git_common_dir: None,
    };
    persist_session_model(&*port, "session-1", &project, "OmniRoute/cursor-auto").await;
    let updates = recorded_models(&port);
    assert_eq!(
        updates,
        vec![(
            "session-1".to_string(),
            Some("OmniRoute/cursor-auto".to_string())
        )]
    );
}
