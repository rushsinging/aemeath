//! Persist the session-selected model into `SessionMetadata.model`.

use share::session_types::ProjectIdentityData;

/// Stable `source/model` key stored on the session for later `--resume`.
pub(crate) fn session_model_key(source_key: &str, model_id: &str) -> String {
    format!("{source_key}/{model_id}")
}

/// Best-effort write of the active model onto the session metadata.
///
/// Failures are logged and swallowed so model persistence never blocks chat.
pub(crate) async fn persist_session_model(
    session_management: &dyn context::SessionManagementPort,
    session_id: &str,
    project: &ProjectIdentityData,
    model: &str,
) {
    let model = model.trim();
    if model.is_empty() {
        return;
    }
    if let Err(error) = session_management
        .update_metadata_for_project(
            session_id,
            project,
            context::SessionMetadataUpdateData {
                model: Some(model.to_string()),
                ..Default::default()
            },
        )
        .await
    {
        log::warn!(
            target: crate::LOG_TARGET,
            "persist session model failed: session={session_id} model={model} error={error}"
        );
    }
}

#[cfg(test)]
#[path = "session_model_persist_tests.rs"]
mod tests;
