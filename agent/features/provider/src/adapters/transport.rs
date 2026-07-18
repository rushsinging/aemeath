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

use crate::domain::invoke::SystemBlock;

use crate::adapters::client::{LlmClient, LlmConfigOptions};
use crate::adapters::pool::LlmClientPool;

use crate::ports::LlmProvider;
use crate::published_language::{InvocationStream, ProviderError};

/// OHS gateway for constructing provider clients and streaming model responses.
#[async_trait]
pub trait LlmProviderGateway: Send + Sync {
    fn client_from_provider(&self, provider: Arc<dyn LlmProvider>) -> LlmClient;

    fn client_from_config(&self, options: LlmConfigOptions) -> Result<LlmClient, crate::LlmError>;

    fn client_pool(
        &self,
        default_client: Arc<LlmClient>,
        models_config: Arc<ModelsConfig>,
        timeout_secs: u64,
    ) -> LlmClientPool;

    async fn invocation_stream(
        &self,
        client: &LlmClient,
        scope: &crate::InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[Value],
        cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError>;
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

    fn client_from_config(&self, options: LlmConfigOptions) -> Result<LlmClient, crate::LlmError> {
        LlmClient::from_config(options)
    }

    fn client_pool(
        &self,
        default_client: Arc<LlmClient>,
        models_config: Arc<ModelsConfig>,
        timeout_secs: u64,
    ) -> LlmClientPool {
        LlmClientPool::new(default_client, models_config, timeout_secs)
    }

    async fn invocation_stream(
        &self,
        client: &LlmClient,
        scope: &crate::InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[Value],
        cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        client
            .invocation_stream(scope, system, messages, tool_schemas, cancel)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures_util::stream;
    use std::sync::Mutex;
    use tokio::sync::{Barrier, Notify};

    fn empty_stream() -> InvocationStream {
        Box::pin(stream::once(async {
            crate::InvocationEvent::Completed(crate::ProviderCompletion {
                output: Vec::new(),
                stop_reason: crate::ProviderStopReason::EndTurn,
                usage: Some(crate::RawUsageSnapshot::default()),
                effective_reasoning: crate::ReasoningLevel::Off,
            })
        }))
    }

    fn scope(
        model: &str,
        max_tokens: u32,
        reasoning: crate::ReasoningLevel,
    ) -> crate::InvocationScope {
        crate::InvocationScope::new(model, max_tokens, reasoning, reasoning).unwrap()
    }

    struct DummyProvider;

    #[async_trait]
    impl LlmProvider for DummyProvider {
        async fn invocation_stream(
            &self,
            _scope: &crate::InvocationScope,
            _system: &[SystemBlock],
            _messages: &[Message],
            _tool_schemas: &[Value],
            _cancel: &CancellationToken,
        ) -> Result<InvocationStream, ProviderError> {
            Ok(empty_stream())
        }

        fn model_name(&self) -> &str {
            "dummy-model"
        }

        fn provider_name(&self) -> &str {
            "dummy"
        }
    }

    #[tokio::test]
    async fn provider_gateway_passes_cancellation_to_in_flight_provider() {
        struct BlockingProvider;

        #[async_trait]
        impl LlmProvider for BlockingProvider {
            async fn invocation_stream(
                &self,
                _scope: &crate::InvocationScope,
                _system: &[SystemBlock],
                _messages: &[Message],
                _tool_schemas: &[Value],
                cancel: &CancellationToken,
            ) -> Result<InvocationStream, ProviderError> {
                cancel.cancelled().await;
                Err(ProviderError::cancelled())
            }

            fn model_name(&self) -> &str {
                "blocking-model"
            }

            fn provider_name(&self) -> &str {
                "blocking"
            }
        }

        let gateway = DefaultLlmProviderGateway;
        let client = gateway.client_from_provider(Arc::new(BlockingProvider));
        let cancel = CancellationToken::new();
        let cancel_task = cancel.clone();
        let canceller = tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancel_task.cancel();
        });

        let scope = crate::InvocationScope::new(
            "blocking-model",
            1024,
            crate::ReasoningLevel::Off,
            crate::ReasoningLevel::Off,
        )
        .unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            gateway.invocation_stream(&client, &scope, &[], &[], &[], &cancel),
        )
        .await
        .expect("取消必须穿过 Gateway 唤醒 Provider future");

        canceller.await.unwrap();
        assert!(matches!(result, Err(error) if error.is_cancelled()));
    }

    #[tokio::test]
    async fn shared_transport_keeps_concurrent_invocation_scopes_isolated() {
        struct ScopeRecordingProvider {
            barrier: Barrier,
            observed: Mutex<Vec<(String, u32, crate::ReasoningLevel)>>,
        }

        #[async_trait]
        impl LlmProvider for ScopeRecordingProvider {
            async fn invocation_stream(
                &self,
                scope: &crate::InvocationScope,
                _system: &[SystemBlock],
                _messages: &[Message],
                _tool_schemas: &[Value],
                _cancel: &CancellationToken,
            ) -> Result<InvocationStream, ProviderError> {
                self.barrier.wait().await;
                self.observed.lock().unwrap().push((
                    scope.model().to_string(),
                    scope.max_tokens(),
                    scope.effective_reasoning(),
                ));
                Ok(empty_stream())
            }

            fn model_name(&self) -> &str {
                "shared-model"
            }

            fn provider_name(&self) -> &str {
                "scope-recorder"
            }
        }

        let provider = Arc::new(ScopeRecordingProvider {
            barrier: Barrier::new(2),
            observed: Mutex::new(Vec::new()),
        });
        let client = Arc::new(LlmClient::from_provider(provider.clone()));
        let first_client = client.clone();
        let second_client = client;
        let first_scope = scope("main-model", 4_096, crate::ReasoningLevel::Low);
        let second_scope = scope("sub-model", 16_384, crate::ReasoningLevel::High);

        let first = tokio::spawn(async move {
            first_client
                .invocation_stream(&first_scope, &[], &[], &[], &CancellationToken::new())
                .await
        });
        let second = tokio::spawn(async move {
            second_client
                .invocation_stream(&second_scope, &[], &[], &[], &CancellationToken::new())
                .await
        });

        let _first_stream = first.await.unwrap().unwrap();
        let _second_stream = second.await.unwrap().unwrap();
        let mut observed = provider.observed.lock().unwrap().clone();
        observed.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(
            observed,
            vec![
                ("main-model".to_string(), 4_096, crate::ReasoningLevel::Low),
                ("sub-model".to_string(), 16_384, crate::ReasoningLevel::High),
            ]
        );
    }

    #[tokio::test]
    async fn cancelling_one_invocation_does_not_affect_another_on_shared_transport() {
        struct CancellationIsolationProvider {
            entered: Barrier,
            survivor_release: Notify,
        }

        #[async_trait]
        impl LlmProvider for CancellationIsolationProvider {
            async fn invocation_stream(
                &self,
                scope: &crate::InvocationScope,
                _system: &[SystemBlock],
                _messages: &[Message],
                _tool_schemas: &[Value],
                cancel: &CancellationToken,
            ) -> Result<InvocationStream, ProviderError> {
                self.entered.wait().await;
                if scope.model() == "cancelled-model" {
                    cancel.cancelled().await;
                    Err(ProviderError::cancelled())
                } else {
                    tokio::select! {
                        () = self.survivor_release.notified() => Ok(empty_stream()),
                        () = cancel.cancelled() => Err(ProviderError::cancelled()),
                    }
                }
            }

            fn model_name(&self) -> &str {
                "shared-model"
            }

            fn provider_name(&self) -> &str {
                "cancellation-isolation"
            }
        }

        let provider = Arc::new(CancellationIsolationProvider {
            entered: Barrier::new(3),
            survivor_release: Notify::new(),
        });
        let client = Arc::new(LlmClient::from_provider(provider.clone()));
        let cancelled_client = client.clone();
        let survivor_client = client;
        let cancelled_token = CancellationToken::new();
        let cancelled_task_token = cancelled_token.clone();
        let survivor_token = CancellationToken::new();

        let cancelled = tokio::spawn(async move {
            cancelled_client
                .invocation_stream(
                    &scope("cancelled-model", 2_048, crate::ReasoningLevel::Medium),
                    &[],
                    &[],
                    &[],
                    &cancelled_task_token,
                )
                .await
        });
        let survivor = tokio::spawn(async move {
            survivor_client
                .invocation_stream(
                    &scope("survivor-model", 8_192, crate::ReasoningLevel::High),
                    &[],
                    &[],
                    &[],
                    &survivor_token,
                )
                .await
        });

        provider.entered.wait().await;
        cancelled_token.cancel();
        assert!(matches!(cancelled.await.unwrap(), Err(error) if error.is_cancelled()));
        provider.survivor_release.notify_one();
        assert!(survivor.await.unwrap().is_ok());
    }

    #[test]
    fn invocation_scopes_reuse_the_same_immutable_client_transport() {
        let client = Arc::new(LlmClient::from_provider(Arc::new(DummyProvider)));
        let main_transport = client.clone();
        let sub_transport = client.clone();
        let main_scope = scope("main-model", 4_096, crate::ReasoningLevel::Low);
        let sub_scope = scope("sub-model", 16_384, crate::ReasoningLevel::High);

        assert!(Arc::ptr_eq(&main_transport, &sub_transport));
        assert_ne!(main_scope, sub_scope);
        assert_eq!(main_transport.provider_name(), "dummy");
        assert_eq!(sub_transport.provider_name(), "dummy");
    }

    #[test]
    fn default_llm_provider_gateway_is_object_safe_and_callable() {
        let gateway: &dyn LlmProviderGateway = &DefaultLlmProviderGateway;
        let client = gateway.client_from_provider(Arc::new(DummyProvider));

        assert_eq!(client.provider_name(), "dummy");
        assert_eq!(client.model_name(), "dummy-model");
    }
}
