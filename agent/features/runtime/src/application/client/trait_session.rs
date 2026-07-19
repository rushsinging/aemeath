//! session 相关方法实际逻辑。

use sdk::{SdkError, SessionSummary};

use super::accessors::AgentClientImpl;
use super::mapping;

pub(super) async fn list_sessions_impl(
    _me: &AgentClientImpl,
) -> Result<Vec<SessionSummary>, SdkError> {
    context::list_session_entries()
        .await
        .map(|sessions| {
            sessions
                .into_iter()
                .map(mapping::session_summary_from_context)
                .collect()
        })
        .map_err(|error| SdkError::Session(error.to_string()))
}
