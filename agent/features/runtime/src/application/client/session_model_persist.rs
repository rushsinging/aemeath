//! Persist the session-selected model into `SessionMetadata.model`.

use share::config::models::ResolvedModel;

#[cfg(test)]
use share::session_types::ProjectIdentityData;

#[cfg(test)]
use super::session_metadata_field::persist_metadata_field;
use super::session_metadata_field::{persist_metadata_field_on_coordinator, SessionMetadataField};

/// Stable `source/model` key stored on the session for later `--resume`.
pub(crate) fn session_model_key(source_key: &str, model_id: &str) -> String {
    format!("{source_key}/{model_id}")
}

/// Best-effort write of the active model onto the session metadata
/// (disk-only path; failures logged and swallowed, never blocks chat).
///
/// Prefer [`persist_session_model_on_wiring`] when a [`SessionSwitchCoordinator`]
/// is available — that path holds `mutation_gate` and syncs `committed_session`.
#[cfg(test)]
pub(crate) async fn persist_session_model(
    session_management: &dyn context::SessionManagementPort,
    session_id: &str,
    project: &ProjectIdentityData,
    model: &str,
) {
    persist_metadata_field(
        session_management,
        session_id,
        project,
        SessionMetadataField::Model,
        model,
    )
    .await;
}

/// Write model onto session metadata, syncing in-memory revision when `session_id`
/// is the coordinator's currently committed active session.
///
/// - Active session → [`SessionSwitchCoordinator::update_active_session_metadata`]
///   (mutation_gate + disk + committed revision sync)。
/// - Other id (bootstrap `--resume` target before `resolve_startup_session`) →
///   disk-only [`persist_session_model`]（`session_management` 由调用方平铺注入）。
pub(crate) async fn persist_session_model_on_wiring(
    coordinator: &crate::SessionSwitchCoordinator,
    session_management: &dyn context::SessionManagementPort,
    session_id: &str,
    model: &str,
) {
    persist_metadata_field_on_coordinator(
        coordinator,
        session_management,
        session_id,
        SessionMetadataField::Model,
        model,
    )
    .await;
}

/// Bootstrap/switch helper: write `source/model` from a resolved binding.
pub(crate) async fn persist_resolved_model_for_session(
    coordinator: &crate::SessionSwitchCoordinator,
    session_management: &dyn context::SessionManagementPort,
    session_id: &str,
    resolved_model: &ResolvedModel,
) {
    persist_session_model_on_wiring(
        coordinator,
        session_management,
        session_id,
        &session_model_key(&resolved_model.source_key, &resolved_model.model.id),
    )
    .await;
}

#[cfg(test)]
#[path = "session_model_persist_tests.rs"]
mod tests;
