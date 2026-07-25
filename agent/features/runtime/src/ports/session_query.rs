//! Main idle query port: Session-scoped queries that belong to the Main
//! Session shell and must not enter RuntimeContext.
//!
//! #1385: Converges four `Arc<Fn>` closures into one typed port.

use async_trait::async_trait;
use sdk::{ModelSummary, ReflectionHistoryView, ReminderView, SdkError, SessionSummary};

/// Session-scoped query port for idle commands (list models, sessions,
/// reminders, reflection history).  Must stay in the Main Session shell;
/// RuntimeContext must NOT reference this port.
#[async_trait]
pub trait SessionQueryPort: Send + Sync {
    async fn list_models(&self) -> Result<Vec<ModelSummary>, SdkError>;
    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SdkError>;
    async fn list_reminders(&self) -> Result<Vec<ReminderView>, SdkError>;
    async fn list_reflection_history(
        &self,
        limit: usize,
    ) -> Result<Vec<ReflectionHistoryView>, SdkError>;
}

#[cfg(test)]
#[path = "session_query_tests.rs"]
mod tests;
