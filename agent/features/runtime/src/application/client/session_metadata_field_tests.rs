use super::*;
use share::session_types::ProjectIdentityData;
use std::sync::Arc;

/// 构造带指定 reasoning_effort 的 session（原 fake 关联 fn 复原）。
fn session_with_reasoning_effort(
    id: &str,
    effort: Option<String>,
) -> context::session::CanonicalSession {
    let mut session = context::session::CanonicalSession::fixture(id);
    session.metadata.reasoning_effort = effort;
    session
}

/// #2146：实现权归 context crate。stored 决定 load 出的 metadata 字段。
fn metadata_port(stored: Option<String>) -> Arc<context::test_support::ScriptedSessionManagement> {
    Arc::new(context::test_support::ScriptedSessionManagement::new(
        move |id| session_with_reasoning_effort(id, stored.clone()),
    ))
}

#[tokio::test]
async fn persist_metadata_field_writes_reasoning_effort() {
    let port = metadata_port(None);
    persist_metadata_field(
        &*port,
        "session-1",
        &ProjectIdentityData::default(),
        SessionMetadataField::ReasoningEffort,
        "high",
    )
    .await;

    let updates = port.updates();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].reasoning_effort.as_deref(), Some("high"));
}

#[tokio::test]
async fn persist_metadata_field_skips_when_value_unchanged() {
    let port = metadata_port(Some("high".to_string()));
    persist_metadata_field(
        &*port,
        "session-1",
        &ProjectIdentityData::default(),
        SessionMetadataField::ReasoningEffort,
        "high",
    )
    .await;

    assert!(port.updates().is_empty());
}

#[tokio::test]
async fn persist_metadata_field_skips_blank_value() {
    let port = metadata_port(None);
    persist_metadata_field(
        &*port,
        "session-1",
        &ProjectIdentityData::default(),
        SessionMetadataField::ReasoningEffort,
        "   ",
    )
    .await;

    assert!(port.updates().is_empty());
}
