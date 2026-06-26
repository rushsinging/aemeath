//! Gateway/OHS for LLM access and model queries.
//!
//! Migration-period exports delegate to the existing client, pool, and provider
//! abstractions without moving provider execution logic.

use async_trait::async_trait;
use serde_json::Value;
use share::config::ModelsConfig;
use share::message::Message;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::business::types::{StreamResponse, SystemBlock};
use crate::LlmError;

pub use crate::core::client::{LlmClient, LlmConfigOptions, OpenAIProviderConfig};
pub use crate::core::pool::LlmClientPool;
pub use crate::core::provider::{CallbackHandler, StreamHandler};

use crate::core::provider::LlmProvider;

/// OHS gateway for constructing provider clients and streaming model responses.
#[async_trait]
pub trait LlmProviderGateway: Send + Sync {
    fn client_from_provider(&self, provider: Arc<dyn LlmProvider>) -> LlmClient;

    fn client_from_config(&self, options: LlmConfigOptions) -> LlmClient;

    fn client_pool(
        &self,
        default_client: Arc<LlmClient>,
        models_config: Arc<ModelsConfig>,
    ) -> LlmClientPool;

    async fn stream_message(
        &self,
        client: &LlmClient,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, LlmError>;
}

/// Default provider gateway backed by the existing provider client/pool API.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultLlmProviderGateway;

pub fn wire_provider() -> Arc<dyn LlmProviderGateway> {
    Arc::new(DefaultLlmProviderGateway)
}

#[async_trait]
impl LlmProviderGateway for DefaultLlmProviderGateway {
    fn client_from_provider(&self, provider: Arc<dyn LlmProvider>) -> LlmClient {
        LlmClient::from_provider(provider)
    }

    fn client_from_config(&self, options: LlmConfigOptions) -> LlmClient {
        LlmClient::from_config(options)
    }

    fn client_pool(
        &self,
        default_client: Arc<LlmClient>,
        models_config: Arc<ModelsConfig>,
    ) -> LlmClientPool {
        LlmClientPool::new(default_client, models_config)
    }

    async fn stream_message(
        &self,
        client: &LlmClient,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, LlmError> {
        client
            .stream_message(system, messages, tool_schemas, handler, cancel)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct DummyProvider;

    #[async_trait]
    impl LlmProvider for DummyProvider {
        async fn stream_message(
            &self,
            _system: &[SystemBlock],
            messages: &[Message],
            _tool_schemas: &[Value],
            _handler: &mut dyn StreamHandler,
            _cancel: &CancellationToken,
        ) -> Result<StreamResponse, LlmError> {
            Ok(StreamResponse {
                assistant_message: messages.last().cloned().unwrap_or_else(|| Message {
                    role: share::message::Role::Assistant,
                    content: Vec::new(),
                    metadata: None,
                }),
                usage: crate::business::types::Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cached_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                    total_tokens: None,
                },
                stop_reason: crate::business::types::StopReason::EndTurn,
            })
        }

        fn model_name(&self) -> &str {
            "dummy-model"
        }

        fn provider_name(&self) -> &str {
            "dummy"
        }
    }

    #[test]
    fn default_llm_provider_gateway_is_object_safe_and_callable() {
        let gateway: &dyn LlmProviderGateway = &DefaultLlmProviderGateway;
        let client = gateway.client_from_provider(Arc::new(DummyProvider));

        assert_eq!(client.provider_name(), "dummy");
        assert_eq!(client.model_name(), "dummy-model");
    }
}
