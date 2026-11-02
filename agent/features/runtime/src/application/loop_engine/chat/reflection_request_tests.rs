//! 反思专用请求参数：三触发共享配置覆盖，未配置时保留模型 binding 默认值。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use memory::api::reflection::ReflectionStatus;
use memory::api::{MemoryPort, ReflectionHistoryQuery, ReflectionHistoryStore};
use share::config::MemoryConfig;
use share::reasoning::ReasoningLevel;
use tokio_util::sync::CancellationToken;

use crate::application::reflection::test_support::RecordingHistory;
use crate::application::reflection::{
    ReflectionRunOutcome, ReflectionTaskAdapter, ReflectionTaskTrigger,
};
use crate::ports::{LlmInvokePort, ProviderBindingData};

type ParameterRecorder = Arc<Mutex<Vec<(ReasoningLevel, u32)>>>;

fn parameter_recording_port(recorder: ParameterRecorder) -> Arc<dyn LlmInvokePort> {
    // #2146：LlmInvokePort 只允许 provider 实现；测试经 provider 脚本化端口。
    provider::test_support::scripted_llm(move |request| {
        recorder
            .lock()
            .unwrap()
            .push((request.reasoning, request.max_output_tokens));
        Ok(provider::test_support::text_stream_with_usage(
            r#"{"deviations":[],"suggested_memories":[],"outdated_memories":[]}"#,
            1,
            1,
        ))
    })
}

fn recording_binding(recorder: ParameterRecorder) -> Arc<ProviderBindingData> {
    Arc::new(ProviderBindingData {
        provider: parameter_recording_port(recorder),
        model: provider::ModelInfo {
            provider: "reflection-parameter-test".to_string(),
            model: "reflection-parameter-model".to_string(),
            supports_tools: false,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            supported_reasoning: vec![
                ReasoningLevel::Off,
                ReasoningLevel::High,
                ReasoningLevel::Max,
            ],
            server_effort_map: Vec::new(),
            context_limit: Some(128_000),
            // 生产装配目前与 binding.max_tokens 同源；不能依靠该字段抬高反思预算。
            output_limit: Some(20_000),
        },
        max_tokens: 20_000,
        requested_reasoning: ReasoningLevel::Max,
    })
}

async fn wait_for_success(history: &RecordingHistory) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let records = history.list(1).await.unwrap();
            if let Some(record) = records.first() {
                if record.status != ReflectionStatus::Running {
                    assert_eq!(record.status, ReflectionStatus::Succeeded);
                    break;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("反思必须在测试时限内完成");
}

async fn invoke_and_record(
    trigger: ReflectionTaskTrigger,
    config: MemoryConfig,
    dedicated_binding: bool,
) -> Vec<(ReasoningLevel, u32)> {
    let recorder: ParameterRecorder = Arc::new(Mutex::new(Vec::new()));
    let binding = recording_binding(Arc::clone(&recorder));
    let mut adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
    if dedicated_binding {
        let mut reflection_binding = (*binding).clone();
        reflection_binding.max_tokens = 32_000;
        reflection_binding.requested_reasoning = ReasoningLevel::Medium;
        reflection_binding.model.output_limit = Some(32_000);
        adapter.set_reflection_binding(Some(Arc::new(reflection_binding)));
    }
    let history = RecordingHistory::default();
    let history_store: Arc<dyn ReflectionHistoryStore> = Arc::new(history.clone());
    let memory: Arc<dyn MemoryPort> = Arc::new(memory::api::NoOpMemory);
    let outcome = super::reflection::run(
        &adapter,
        trigger,
        &config,
        vec![share::message::Message::user("反思这段会话")],
        &binding,
        "system",
        "zh",
        &memory,
        &history_store,
        None,
        CancellationToken::new(),
        None,
    )
    .await;
    assert!(matches!(outcome, ReflectionRunOutcome::Detached));
    wait_for_success(&history).await;
    assert_eq!(binding.max_tokens, 20_000, "主对话预算不能被反思配置改写");
    assert_eq!(binding.requested_reasoning, ReasoningLevel::Max);
    let requests = recorder.lock().unwrap().clone();
    requests
}

#[tokio::test]
async fn reflection_request_overrides_reach_both_phases_for_all_triggers() {
    let config: MemoryConfig =
        serde_json::from_str(r#"{"reflection":{"reasoning":"high","max_tokens":64000}}"#).unwrap();
    for trigger in [
        ReflectionTaskTrigger::Interval { run_ordinal: 1 },
        ReflectionTaskTrigger::PreCompact,
        ReflectionTaskTrigger::Manual,
    ] {
        for dedicated_binding in [false, true] {
            let requests = invoke_and_record(trigger, config.clone(), dedicated_binding).await;
            assert_eq!(
                requests,
                vec![(ReasoningLevel::High, 64_000); 2],
                "{trigger:?} dedicated_binding={dedicated_binding} 的去噪和归纳都必须消费覆盖参数"
            );
        }
    }
}

#[tokio::test]
async fn reflection_request_without_overrides_inherits_binding_parameters() {
    for trigger in [
        ReflectionTaskTrigger::Interval { run_ordinal: 1 },
        ReflectionTaskTrigger::PreCompact,
        ReflectionTaskTrigger::Manual,
    ] {
        for dedicated_binding in [false, true] {
            let requests =
                invoke_and_record(trigger, MemoryConfig::default(), dedicated_binding).await;
            let expected = if dedicated_binding {
                (ReasoningLevel::Medium, 32_000)
            } else {
                (ReasoningLevel::Max, 20_000)
            };
            assert_eq!(requests, vec![expected; 2]);
        }
    }
}

#[tokio::test]
async fn reflection_request_preserves_explicit_off_and_independent_overrides() {
    for (json, expected) in [
        (
            r#"{"reflection":{"reasoning":"off"}}"#,
            (ReasoningLevel::Off, 20_000),
        ),
        (
            r#"{"reflection":{"max_tokens":64000}}"#,
            (ReasoningLevel::Max, 64_000),
        ),
    ] {
        let config = serde_json::from_str(json).unwrap();
        let requests = invoke_and_record(ReflectionTaskTrigger::Manual, config, false).await;
        assert_eq!(requests, vec![expected; 2]);
    }
}
