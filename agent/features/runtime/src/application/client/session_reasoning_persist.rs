//! Persist the session-selected reasoning effort into
//! `SessionMetadata.reasoning_effort`（#2032）.

use share::reasoning::ReasoningLevel;

use super::session_metadata_field::{persist_metadata_field_on_coordinator, SessionMetadataField};

/// Best-effort write of the `/think`-selected level onto the session metadata.
///
/// Failures are logged and swallowed so reasoning persistence never blocks chat.
pub(crate) async fn persist_session_reasoning_on_wiring(
    coordinator: &crate::SessionSwitchCoordinator,
    session_management: &dyn context::SessionManagementPort,
    session_id: &str,
    level: ReasoningLevel,
) {
    persist_metadata_field_on_coordinator(
        coordinator,
        session_management,
        session_id,
        SessionMetadataField::ReasoningEffort,
        level.as_str(),
    )
    .await;
}
