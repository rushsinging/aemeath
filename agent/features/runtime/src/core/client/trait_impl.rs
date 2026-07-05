//! AgentClient trait 实现 — 薄委托到各子模块。

use async_trait::async_trait;
use sdk::{
    AgentClient, ChatRequest, ChatStream, ModelSummary, SdkError, SessionSnapshot, SessionSummary,
};

use super::accessors::AgentClientImpl;

#[async_trait]
impl AgentClient for AgentClientImpl {
    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, SdkError> {
        super::trait_chat::chat_impl(self, input).await
    }

    async fn load_session(&self, id: &str) -> Result<SessionSnapshot, SdkError> {
        super::trait_session::load_session_impl(self, id).await
    }

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SdkError> {
        super::trait_session::list_sessions_impl(self).await
    }

    async fn delete_session(&self, id: &str) -> Result<(), SdkError> {
        super::trait_session::delete_session_impl(self, id).await
    }

    async fn list_models(&self) -> Result<Vec<ModelSummary>, SdkError> {
        super::trait_model::list_models_impl(self).await
    }
}
