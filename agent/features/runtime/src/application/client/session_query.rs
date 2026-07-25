//! `AgentClientImpl` adapter for `SessionQueryPort`.
//!
//! This adapter belongs to the Main Session shell and must NOT be referenced
//! by `RuntimeContext`. Each method delegates to an existing `*_impl` function.

use async_trait::async_trait;
use sdk::{ModelSummary, ReflectionHistoryView, ReminderView, SdkError, SessionSummary};
use std::sync::Arc;

use super::accessors::AgentClientImpl;
use crate::ports::SessionQueryPort;

/// Adapter that wraps `Arc<AgentClientImpl>` to satisfy `SessionQueryPort`.
pub(super) struct AgentSessionQuery {
    client: Arc<AgentClientImpl>,
}

impl AgentSessionQuery {
    pub fn new(client: Arc<AgentClientImpl>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl SessionQueryPort for AgentSessionQuery {
    async fn list_models(&self) -> Result<Vec<ModelSummary>, SdkError> {
        super::trait_model::list_models_impl(&self.client).await
    }

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SdkError> {
        super::trait_session::list_sessions_impl(&self.client).await
    }

    async fn list_reminders(&self) -> Result<Vec<ReminderView>, SdkError> {
        super::trait_memory::list_reminders_impl(&self.client).await
    }

    async fn list_reflection_history(
        &self,
        limit: usize,
    ) -> Result<Vec<ReflectionHistoryView>, SdkError> {
        super::trait_reflection::list_reflection_history_impl(&self.client, limit).await
    }
}
