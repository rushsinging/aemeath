//! Persist the session-selected model into `SessionMetadata.model`.

use share::config::models::ResolvedModel;
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
    match session_management
        .load_for_project(session_id, project)
        .await
    {
        Ok(session) => {
            if session.metadata.model.as_deref() == Some(model) {
                return;
            }
        }
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "persist session model load failed (continuing write): session={session_id} error={error}"
            );
        }
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

/// Bootstrap/switch helper: write `source/model` from a resolved binding.
pub(crate) async fn persist_resolved_model_for_session(
    wiring: &context::MainSessionWiring,
    session_id: &str,
    resolved_model: &ResolvedModel,
) {
    persist_session_model(
        wiring.session_management().as_ref(),
        session_id,
        &wiring.project_identity(),
        &session_model_key(&resolved_model.source_key, &resolved_model.model.id),
    )
    .await;
}

#[cfg(test)]
#[path = "session_model_persist_tests.rs"]
mod tests;
