//! AgentClient trait 实现 — 薄委托到各子模块。

use async_trait::async_trait;
use sdk::{AgentClient, ChatRequest, ChatStream, SdkError};

use super::accessors::AgentClientImpl;

#[async_trait]
impl AgentClient for AgentClientImpl {
    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, SdkError> {
        super::trait_chat::chat_impl(self, input).await
    }
}
