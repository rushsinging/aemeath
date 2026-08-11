//! session 相关方法实际逻辑。

use sdk::{DisplayHistoryWindowRequest, SdkError, SessionSummary};

use super::accessors::AgentClientImpl;
use super::mapping;

pub(super) async fn load_display_history_window_impl(
    me: &AgentClientImpl,
    request: DisplayHistoryWindowRequest,
) -> Result<sdk::DisplayHistoryWindow, SdkError> {
    let session_management = me.inner.shell.session_management.clone();
    let project = me.inner.shell.wiring.project_identity();
    let window = me
        .inner
        .shell
        .wiring
        .with_shared(async move {
            session_management
                .load_display_history_steps(
                    &request.session_id,
                    &project,
                    request.generation_revision,
                    &request.member_names,
                )
                .await
        })
        .await
        .map_err(|error| SdkError::Session(error.to_string()))?
        .map_err(|error| SdkError::Session(error.to_string()))?;
    Ok(mapping::display_history_window_to_sdk(window))
}

pub(super) async fn list_sessions_impl(
    me: &AgentClientImpl,
) -> Result<Vec<SessionSummary>, SdkError> {
    let session_management = me.inner.shell.session_management.clone();
    let project = me.inner.shell.wiring.project_identity();
    let sessions = me
        .inner
        .shell
        .wiring
        .with_shared(async move { session_management.list_for_project(&project).await })
        .await
        .map_err(|error| SdkError::Session(error.to_string()))?
        .map_err(|error| SdkError::Session(error.to_string()))?;
    Ok(sessions
        .into_iter()
        .map(mapping::session_summary_from_context)
        .collect())
}
