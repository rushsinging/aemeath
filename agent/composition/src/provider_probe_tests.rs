use super::*;
use async_trait::async_trait;
use config::connect::{ProviderProbeErrorKind, ProviderProbePort, ProviderProbeRequest};
use futures_util::stream;
use provider::composition::{InvocationScope, LlmClient, LlmProvider, SystemBlock};
use provider::{
    InvocationEvent, ProviderCompletion, ProviderContentBlock, ProviderError, ProviderErrorKind,
    ProviderStopReason, ReasoningLevel,
};
use share::message::Message;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct CapturedInvocation {
    count: usize,
    max_tokens: Option<u32>,
}

struct EventProvider {
    events: Vec<InvocationEvent>,
    captured: Arc<Mutex<CapturedInvocation>>,
}

#[async_trait]
impl LlmProvider for EventProvider {
    async fn invocation_stream(
        &self,
        scope: &InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tools: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<provider::InvocationStream, ProviderError> {
        let mut captured = self.captured.lock().unwrap();
        captured.count += 1;
        captured.max_tokens = Some(scope.max_tokens());
        Ok(Box::pin(stream::iter(self.events.clone())))
    }

    fn model_name(&self) -> &str {
        "probe-model"
    }

    fn provider_name(&self) -> &str {
        "Anthropic"
    }
}

#[derive(Clone)]
struct CapturedClientSpec {
    driver: String,
    api_key: String,
    base_url: Option<String>,
    model: String,
    max_tokens: u32,
    timeout_secs: u64,
    user_agent: String,
}

struct FakeProbeClientFactory {
    client: Arc<LlmClient>,
    specs: Arc<Mutex<Vec<CapturedClientSpec>>>,
}

impl ProbeClientFactory for FakeProbeClientFactory {
    fn build(&self, spec: ProbeClientSpec) -> Result<Arc<LlmClient>, ProviderError> {
        self.specs.lock().unwrap().push(CapturedClientSpec {
            driver: spec.driver,
            api_key: spec.api_key,
            base_url: spec.base_url,
            model: spec.model,
            max_tokens: spec.max_tokens,
            timeout_secs: spec.timeout_secs,
            user_agent: spec.user_agent,
        });
        Ok(self.client.clone())
    }
}

fn completed() -> InvocationEvent {
    InvocationEvent::Completed(ProviderCompletion {
        output: vec![ProviderContentBlock::Text("OK".to_string())],
        stop_reason: ProviderStopReason::EndTurn,
        usage: None,
        effective_reasoning: ReasoningLevel::Off,
    })
}

fn request() -> ProviderProbeRequest {
    ProviderProbeRequest {
        driver: config::catalog::find_by_source("Anthropic").unwrap().driver,
        base_url: "https://probe.test".to_string(),
        credential: Some("sk-probe-secret".to_string()),
        model_id: "probe-model".to_string(),
        context_window: 32_000,
        max_tokens: 64,
        final_user_agent: "probe-agent/1.0".to_string(),
        timeout: std::time::Duration::from_secs(9),
    }
}

struct ProbeFixture {
    adapter: ProviderProbeAdapter,
    specs: Arc<Mutex<Vec<CapturedClientSpec>>>,
    invocation: Arc<Mutex<CapturedInvocation>>,
}

fn adapter(events: Vec<InvocationEvent>) -> ProbeFixture {
    let invocation = Arc::new(Mutex::new(CapturedInvocation::default()));
    let client = Arc::new(LlmClient::from_provider(Arc::new(EventProvider {
        events,
        captured: invocation.clone(),
    })));
    let specs = Arc::new(Mutex::new(Vec::new()));
    let factory = Arc::new(FakeProbeClientFactory {
        client,
        specs: specs.clone(),
    });
    ProbeFixture {
        adapter: ProviderProbeAdapter::with_factory(factory),
        specs,
        invocation,
    }
}

#[tokio::test]
async fn probe_maps_request_and_invokes_once() {
    let fixture = adapter(vec![completed()]);
    fixture.adapter.probe(request()).await.unwrap();

    let specs = fixture.specs.lock().unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].driver, "anthropic");
    assert_eq!(specs[0].api_key, "sk-probe-secret");
    assert_eq!(specs[0].base_url.as_deref(), Some("https://probe.test"));
    assert_eq!(specs[0].model, "probe-model");
    assert_eq!(specs[0].max_tokens, 1);
    assert_eq!(specs[0].timeout_secs, 9);
    assert_eq!(specs[0].user_agent, "probe-agent/1.0");
    let invocation = fixture.invocation.lock().unwrap();
    assert_eq!(invocation.count, 1);
    assert_eq!(invocation.max_tokens, Some(1));
}

#[tokio::test]
async fn probe_requires_completed_terminal_event() {
    let fixture = adapter(Vec::new());
    let error = fixture.adapter.probe(request()).await.unwrap_err();
    assert_eq!(error.kind, ProviderProbeErrorKind::Protocol);
}

#[tokio::test]
async fn probe_maps_failed_event_without_sensitive_message() {
    let secret = "sk-probe-secret";
    let failed = InvocationEvent::Failed(ProviderError::fatal(
        ProviderErrorKind::Authentication,
        format!("Authorization Bearer {secret}"),
    ));
    let fixture = adapter(vec![failed]);
    let error = fixture.adapter.probe(request()).await.unwrap_err();
    assert_eq!(error.kind, ProviderProbeErrorKind::Authentication);
    assert!(!error.message.contains(secret));
    assert!(!error.message.contains("Authorization"));
}

#[test]
fn provider_error_mapping_is_stable_and_redacted() {
    let cases = [
        (
            ProviderErrorKind::Cancelled,
            ProviderProbeErrorKind::Cancelled,
        ),
        (ProviderErrorKind::Timeout, ProviderProbeErrorKind::Timeout),
        (
            ProviderErrorKind::Authentication,
            ProviderProbeErrorKind::Authentication,
        ),
        (
            ProviderErrorKind::PermissionDenied,
            ProviderProbeErrorKind::Authentication,
        ),
        (
            ProviderErrorKind::ModelUnavailable,
            ProviderProbeErrorKind::Model,
        ),
        (
            ProviderErrorKind::Protocol,
            ProviderProbeErrorKind::Protocol,
        ),
        (ProviderErrorKind::Network, ProviderProbeErrorKind::Endpoint),
    ];
    for (source, expected) in cases {
        let error = map_probe_error(ProviderError::fatal(source, "sensitive wire body"));
        assert_eq!(error.kind, expected);
        assert!(!error.message.contains("wire body"));
    }
}
