//! Scenario tests for the reflection memory-update notice (double channel).
//!
//! A finished reflection that changed memory must reach the TUI at the moment it
//! completes, and leave one LLM reminder behind for the next Run to take. A run
//! without changes, a failed run, and a configuration skip must produce neither.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::reflection::{announce_memory_update, memory_updated_notice_text};
use crate::application::loop_engine::chat::{
    ChatEventSink, ChatEventSinkHandle, EventFuture, RuntimeStreamEvent,
};
use crate::application::reflection::{ReflectionRunOutcome, ReflectionTaskAdapter};

/// Sink that keeps only system messages, which is the whole contract here.
#[derive(Clone, Default)]
struct SystemMessageSink {
    messages: Arc<Mutex<Vec<String>>>,
}

impl SystemMessageSink {
    fn handle(&self) -> ChatEventSinkHandle {
        ChatEventSinkHandle::new(self.clone())
    }

    fn messages(&self) -> Vec<String> {
        self.messages.lock().expect("sink lock").clone()
    }
}

impl ChatEventSink for SystemMessageSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        self.try_send_event(event);
        Box::pin(std::future::ready(()))
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        if let RuntimeStreamEvent::SystemMessage(message) = event {
            self.messages.lock().expect("sink lock").push(message);
        }
    }
}

struct ReflectionProvider {
    response: String,
}

#[async_trait::async_trait]
impl crate::ports::ProviderPort for ReflectionProvider {
    // `capabilities()` 已删除（#1880）：binding 持全量 ModelInfo，运行时零查询。

    async fn invoke(
        &self,
        _request: crate::ports::provider_port::InvocationRequestData,
        _cancel: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::ProviderResponseStream,
        crate::ports::provider_port::ProviderError,
    > {
        Ok(
            crate::application::model::test_support::text_completion_stream(
                self.response.clone(),
                1,
                1,
            ),
        )
    }
}

fn binding(provider: Arc<ReflectionProvider>) -> Arc<crate::ports::ProviderBindingData> {
    Arc::new(crate::ports::ProviderBindingData {
        provider,
        model: provider::ModelInfo {
            provider: "notice-test".to_string(),
            model: "notice-test-model".to_string(),
            supports_tools: false,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            supported_reasoning: vec![share::reasoning::ReasoningLevel::Off],
            context_limit: Some(128_000),
            output_limit: Some(8_192),
        },
        max_tokens: 8_192,
        requested_reasoning: share::reasoning::ReasoningLevel::Off,
    })
}

fn provider_proposing(count: usize) -> Arc<ReflectionProvider> {
    let suggestions = (0..count)
        .map(|index| {
            format!(
                r#"{{"layer":"project","category":"decision","content":"proposed entry {index}","reason":"because"}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Arc::new(ReflectionProvider {
        response: format!(
            r#"{{"deviations":[],"suggested_memories":[{suggestions}],"outdated_memories":[]}}"#
        ),
    })
}

fn provider_proposing_nothing() -> Arc<ReflectionProvider> {
    Arc::new(ReflectionProvider {
        response: r#"{"deviations":[],"suggested_memories":[],"outdated_memories":[]}"#.to_string(),
    })
}

fn auto_apply_config() -> share::config::MemoryConfig {
    share::config::MemoryConfig {
        enabled: true,
        reflection: share::config::ReflectionConfig {
            enabled: true,
            auto_apply_suggestions: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn disabled_config() -> share::config::MemoryConfig {
    share::config::MemoryConfig {
        enabled: false,
        reflection: share::config::ReflectionConfig {
            enabled: true,
            auto_apply_suggestions: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn in_memory() -> Arc<dyn memory::api::MemoryPort> {
    Arc::new(
        memory::api::InMemoryMemory::new(memory::api::MemoryPolicy::default())
            .expect("the default in-memory policy must be valid"),
    )
}

async fn run_with(
    provider: Arc<ReflectionProvider>,
    config: share::config::MemoryConfig,
) -> (ReflectionTaskAdapter, ReflectionRunOutcome) {
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
    let outcome = super::reflection::run(
        &adapter,
        crate::application::reflection::ReflectionTaskTrigger::Manual,
        &config,
        vec![share::message::Message::user(
            "a conversation worth reflecting on",
        )],
        &binding(provider),
        "system",
        "en",
        &in_memory(),
        &crate::application::reflection::test_support::noop_reflection_history(),
        None,
        tokio_util::sync::CancellationToken::new(),
    )
    .await;
    (adapter, outcome)
}

#[test]
fn the_notice_text_states_the_count_in_both_languages() {
    assert_eq!(memory_updated_notice_text(3, "zh"), "记忆已更新 3 条");
    assert_eq!(
        memory_updated_notice_text(1, "en"),
        "Memory updated: 1 entry"
    );
    assert_eq!(
        memory_updated_notice_text(2, "en"),
        "Memory updated: 2 entries"
    );
}

#[tokio::test]
async fn a_changed_memory_announces_the_tui_notice_and_arms_one_llm_reminder() {
    let (adapter, outcome) = run_with(provider_proposing(2), auto_apply_config()).await;
    let sink = SystemMessageSink::default();

    let announced = announce_memory_update(&sink.handle(), &outcome, "en").await;

    assert_eq!(announced, Some(2), "the run changed two entries");
    assert_eq!(sink.messages(), ["Memory updated: 2 entries"]);
    assert_eq!(
        adapter.take_memory_update_notice(),
        Some(crate::application::reflection::MemoryUpdateNotice { changed: 2 }),
        "the next Run must find exactly one pending reminder"
    );
    assert_eq!(
        adapter.take_memory_update_notice(),
        None,
        "the reminder must not be injected twice"
    );
}

#[tokio::test]
async fn a_run_without_changes_stays_silent_on_both_channels() {
    let (adapter, outcome) = run_with(provider_proposing_nothing(), auto_apply_config()).await;
    let sink = SystemMessageSink::default();

    assert_eq!(
        announce_memory_update(&sink.handle(), &outcome, "en").await,
        None
    );
    assert!(sink.messages().is_empty());
    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn a_disabled_configuration_stays_silent_on_both_channels() {
    let (adapter, outcome) = run_with(provider_proposing(2), disabled_config()).await;
    let sink = SystemMessageSink::default();

    assert!(matches!(outcome, ReflectionRunOutcome::DisabledSkipped));
    assert_eq!(
        announce_memory_update(&sink.handle(), &outcome, "en").await,
        None
    );
    assert!(sink.messages().is_empty());
    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn two_runs_before_the_next_one_merge_into_a_single_reminder() {
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
    let history = crate::application::reflection::test_support::noop_reflection_history();
    for _ in 0..2 {
        super::reflection::run(
            &adapter,
            crate::application::reflection::ReflectionTaskTrigger::Manual,
            &auto_apply_config(),
            vec![share::message::Message::user(
                "a conversation worth reflecting on",
            )],
            &binding(provider_proposing(1)),
            "system",
            "en",
            &in_memory(),
            &history,
            None,
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
    }

    assert_eq!(
        adapter.take_memory_update_notice(),
        Some(crate::application::reflection::MemoryUpdateNotice { changed: 2 }),
    );
}
