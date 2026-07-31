use super::*;
use crate::application::model::test_support::text_completion_stream;
use crate::ports::provider_port::{
    InvocationRequest, InvocationStream, ModelCapability, ModelId, ProviderError,
    ProviderErrorKind, ReasoningCapability,
};
use async_trait::async_trait;
use memory::api::{
    MemoryError, NoOpMemory, ReflectionHistoryQuery, ReflectionRecord, ReflectionSafeSummary,
};
use std::sync::Mutex;

struct StaticProvider {
    response: String,
}

#[async_trait]
impl ProviderPort for StaticProvider {
    fn capabilities(&self, model: &ModelId) -> Result<ModelCapability, ProviderError> {
        if model.provider != "reflection-test-provider" {
            return Err(ProviderError::fatal(
                ProviderErrorKind::ModelUnavailable,
                format!("unknown model: {model}"),
            ));
        }
        Ok(ModelCapability {
            model: model.clone(),
            supports_tools: false,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            reasoning: ReasoningCapability::none(),
            context_limit: Some(8_192),
            output_limit: Some(4_096),
        })
    }

    async fn invoke(
        &self,
        _request: InvocationRequest,
        _cancel: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<InvocationStream, ProviderError> {
        Ok(text_completion_stream(self.response.clone(), 11, 22))
    }
}

#[derive(Default)]
struct RecordingHistory {
    records: Mutex<Vec<ReflectionRecord>>,
}

#[async_trait]
impl ReflectionHistoryQuery for RecordingHistory {
    async fn list(&self, limit: usize) -> Result<Vec<ReflectionSafeSummary>, MemoryError> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .rev()
            .take(limit)
            .map(ReflectionRecord::safe_summary)
            .collect())
    }
}

#[async_trait]
impl ReflectionHistoryStore for RecordingHistory {
    async fn append(&self, record: &ReflectionRecord) -> Result<(), MemoryError> {
        self.records.lock().unwrap().push(record.clone());
        Ok(())
    }

    async fn upsert(&self, record: &ReflectionRecord) -> Result<(), MemoryError> {
        let mut records = self.records.lock().unwrap();
        if let Some(existing) = records.iter_mut().find(|item| item.id == record.id) {
            *existing = record.clone();
        } else {
            records.push(record.clone());
        }
        Ok(())
    }
}

fn model() -> ModelId {
    ModelId {
        provider: "reflection-test-provider".to_string(),
        model: "reflection-test-model".to_string(),
    }
}

fn identity() -> ReflectionExecutionIdentity {
    ReflectionExecutionIdentity {
        id: "reflection-id".to_string(),
        timestamp: 42,
        trigger: memory::api::ReflectionTrigger::Manual,
    }
}

#[tokio::test]
async fn runtime_invokes_provider_then_delegates_parse_apply_and_history_to_memory() {
    let provider = StaticProvider {
        response: r#"{"deviations":["drift"],"suggested_memories":[]}"#.to_string(),
    };
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let model = model();
    let result = execute_reflection(
        &[share::message::Message::user("reflect")],
        "en",
        false,
        ReflectionInvocation {
            provider: &provider,
            model: &model,
            max_tokens: 4_096,
            requested_reasoning: provider::ReasoningLevel::Off,
            system_prompt_text: "system",
        },
        &NoOpMemory,
        &history,
        &identity(),
        &cancel,
    )
    .await
    .unwrap();

    assert_eq!(result.output.deviations, ["drift"]);
    assert_eq!((result.input_tokens, result.output_tokens), (11, 22));
    assert_eq!(history.list(1).await.unwrap()[0].id, "reflection-id");
}

#[tokio::test]
async fn malformed_provider_text_returns_safe_runtime_error_and_memory_records_parse_failure() {
    let secret = "SECRET-provider-raw-response";
    let provider = StaticProvider {
        response: secret.to_string(),
    };
    let history = RecordingHistory::default();
    let cancel = tokio_util::sync::CancellationToken::new();
    let model = model();
    let error = execute_reflection(
        &[],
        "en",
        false,
        ReflectionInvocation {
            provider: &provider,
            model: &model,
            max_tokens: 4_096,
            requested_reasoning: provider::ReasoningLevel::Off,
            system_prompt_text: "system",
        },
        &NoOpMemory,
        &history,
        &identity(),
        &cancel,
    )
    .await
    .unwrap_err();

    assert_eq!(error, ReflectionExecutionError::Unparseable);
    assert!(!error.to_string().contains(secret));
    assert_eq!(
        history.list(1).await.unwrap()[0].error_category,
        Some(ReflectionErrorCategory::Parse)
    );
}
