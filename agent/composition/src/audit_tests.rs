use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use runtime::UsageSink;

use super::*;

#[derive(Clone)]
struct NoChatClient;

#[async_trait]
impl sdk::AgentClient for NoChatClient {
    async fn chat(&self, _input: sdk::ChatRequest) -> Result<sdk::ChatStream, sdk::SdkError> {
        Err(sdk::SdkError::Internal("测试不发起 chat".to_string()))
    }
}

#[tokio::test]
async fn audit_lifecycle_client_clones_share_one_idempotent_shutdown() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = storage::SafeStorageRoot::open(temp.path()).expect("storage root");
    let store = Arc::new(audit::file_usage_append_store(root));
    let (sender, handle) = audit::start_usage_worker(
        store,
        audit::UsageWorkerConfig::new(4, Duration::from_secs(1)),
    );
    let sink = AuditUsageSink::new(sender);
    let client = AuditLifecycleClient::new(Arc::new(NoChatClient), handle);
    let clone = client.clone();

    let first = sdk::AgentClient::shutdown(&client).await;
    let second = sdk::AgentClient::shutdown(&clone).await;

    assert_eq!(first, sdk::ClientShutdownOutcome::Drained);
    assert_eq!(second, first);
    let record = audit::UsageRecord {
        recorded_at_unix_ms: 1,
        session_id: sdk::SessionId::new("01900000-0000-7000-8000-000000000021"),
        run_id: sdk::RunId::new("01900000-0000-7000-8000-000000000022"),
        run_step_id: sdk::RunStepId::new("01900000-0000-7000-8000-000000000023"),
        model_invocation_id: sdk::ModelInvocationId::new("01900000-0000-7000-8000-000000000024"),
        provider: "provider".to_string(),
        model: "model".to_string(),
        input_tokens: 1,
        output_tokens: 1,
        cache_write_tokens: None,
        cache_read_tokens: None,
        reasoning_tokens: None,
    };
    assert_eq!(
        sink.try_record(record),
        audit::UsageEmitOutcome::Dropped(audit::UsageDropReason::WorkerUnavailable)
    );
}
