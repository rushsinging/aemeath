//! session 相关方法实际逻辑。

use sdk::{SdkError, SessionSummary};

use super::accessors::AgentClientImpl;
use super::mapping;

pub(super) async fn list_sessions_impl(
    me: &AgentClientImpl,
) -> Result<Vec<SessionSummary>, SdkError> {
    let session_management = me.inner.session_management.clone();
    let project = me.inner.wiring.project_identity();
    let sessions = me
        .inner
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
