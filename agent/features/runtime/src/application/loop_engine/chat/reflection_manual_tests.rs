//! External tests for the Manual reflection trigger.
//!
//! `/reflect-now` 在 idle 路径受理为 Manual Reflection Run；执行结果回显文案：只有配置禁用
//! 是显式跳过，执行失败按 `is_error` 上报，其余为正常完成。

use std::sync::Arc;
use std::time::Duration;

use crate::application::loop_engine::chat::reflection::{
    manual_outcome_notice, manual_reflection_outcome_text, ManualReflectionNotice,
};
use crate::application::reflection::{
    ReflectionRunOutcome, ReflectionTaskAdapter, ReflectionTaskCompletion,
    ReflectionTaskCompletionStatus,
};
use share::message::Message;

fn enabled_memory_config() -> share::config::MemoryConfig {
    share::config::MemoryConfig {
        enabled: true,
        reflection: share::config::ReflectionConfig {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn fake_binding() -> Arc<crate::ports::ProviderBindingData> {
    Arc::new(crate::ports::ProviderBindingData {
        provider: Arc::new(crate::application::reflection::test_support::StaticReflectionProvider),
        model: provider::ModelInfo {
            provider: "reflection-test".to_string(),
            model: "manual-test-model".to_string(),
            supports_tools: false,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            reasoning: provider::ReasoningCapabilityData::none(),
            context_limit: Some(128_000),
            output_limit: Some(8_192),
        },
        max_tokens: 8_192,
        requested_reasoning: share::reasoning::ReasoningLevel::Off,
    })
}

fn completed(status: ReflectionTaskCompletionStatus, changed: usize) -> ReflectionTaskCompletion {
    ReflectionTaskCompletion {
        trigger: crate::application::reflection::ReflectionTaskTrigger::Manual,
        status,
        metadata: Some(crate::application::reflection::ReflectionTaskMetadata {
            error_category: None,
            input_tokens: 0,
            output_tokens: 0,
            deviations: 0,
            suggestions: 0,
            outdated: 0,
            suggestions_added: changed,
            outdated_marked: 0,
            superseded: 0,
            duration_ms: 1,
            record_id: None,
        }),
    }
}

#[test]
fn manual_outcome_text_reports_disabled_without_error() {
    let (text, is_error) = manual_reflection_outcome_text(&ReflectionRunOutcome::DisabledSkipped);
    assert!(!is_error);
    assert_eq!(text, "Memory 或 Reflection 未启用；请在配置中开启后重试。");
}

#[test]
fn manual_outcome_text_reports_the_completed_change_count() {
    let (text, is_error) = manual_reflection_outcome_text(&ReflectionRunOutcome::Completed(
        completed(ReflectionTaskCompletionStatus::Succeeded, 3),
    ));
    assert!(!is_error);
    assert_eq!(
        text,
        "Reflection 已完成：更新 3 条记忆；摘要可用 /reflect 查询。"
    );
}

#[test]
fn manual_outcome_text_reports_zero_changes_without_a_count() {
    let (text, is_error) = manual_reflection_outcome_text(&ReflectionRunOutcome::Completed(
        completed(ReflectionTaskCompletionStatus::Succeeded, 0),
    ));
    assert!(!is_error);
    assert_eq!(text, "Reflection 已完成：没有记忆变更。");
}

#[test]
fn only_a_failed_run_reports_error_semantics() {
    let (text, is_error) = manual_reflection_outcome_text(&ReflectionRunOutcome::Completed(
        completed(ReflectionTaskCompletionStatus::Failed, 0),
    ));
    assert!(is_error);
    assert_eq!(text, "Reflection 执行失败；详情见日志。");

    for status in [
        ReflectionTaskCompletionStatus::Succeeded,
        ReflectionTaskCompletionStatus::Cancelled,
        ReflectionTaskCompletionStatus::TimedOut,
    ] {
        let (_, is_error) =
            manual_reflection_outcome_text(&ReflectionRunOutcome::Completed(completed(status, 0)));
        assert!(!is_error, "{status:?} is not an execution error");
    }
}

/// 取消与超时是正常终态：各自有独立的用户解释文案，且都不得按错误样式发布。
#[test]
fn manual_outcome_text_explains_cancellation_and_timeout_without_error() {
    let (cancelled_text, cancelled_is_error) = manual_reflection_outcome_text(
        &ReflectionRunOutcome::Completed(completed(ReflectionTaskCompletionStatus::Cancelled, 0)),
    );
    assert!(!cancelled_is_error);
    assert_eq!(cancelled_text, "Reflection 已取消。");

    let (timed_out_text, timed_out_is_error) = manual_reflection_outcome_text(
        &ReflectionRunOutcome::Completed(completed(ReflectionTaskCompletionStatus::TimedOut, 0)),
    );
    assert!(!timed_out_is_error);
    assert_eq!(timed_out_text, "Reflection 超时，已中止。");
}

/// 文案与 `is_error` 必须由同一条终态映射策略成对产出：六种终态各自的
/// `(text, is_error)` 精确落表，且外部 `(String, bool)` 入口只是该策略的投影——
/// 两者永远不会分叉。
#[test]
fn manual_outcome_notice_pairs_text_and_error_from_one_policy() {
    let expected_states: [(&ReflectionRunOutcome, &str, bool); 6] = [
        (
            &ReflectionRunOutcome::DisabledSkipped,
            "Memory 或 Reflection 未启用；请在配置中开启后重试。",
            false,
        ),
        (
            &ReflectionRunOutcome::Completed(completed(
                ReflectionTaskCompletionStatus::Succeeded,
                3,
            )),
            "Reflection 已完成：更新 3 条记忆；摘要可用 /reflect 查询。",
            false,
        ),
        (
            &ReflectionRunOutcome::Completed(completed(
                ReflectionTaskCompletionStatus::Succeeded,
                0,
            )),
            "Reflection 已完成：没有记忆变更。",
            false,
        ),
        (
            &ReflectionRunOutcome::Completed(completed(ReflectionTaskCompletionStatus::Failed, 0)),
            "Reflection 执行失败；详情见日志。",
            true,
        ),
        (
            &ReflectionRunOutcome::Completed(completed(
                ReflectionTaskCompletionStatus::Cancelled,
                0,
            )),
            "Reflection 已取消。",
            false,
        ),
        (
            &ReflectionRunOutcome::Completed(completed(
                ReflectionTaskCompletionStatus::TimedOut,
                0,
            )),
            "Reflection 超时，已中止。",
            false,
        ),
    ];

    for (outcome, expected_text, expected_is_error) in expected_states {
        let notice = manual_outcome_notice(outcome);
        assert_eq!(
            notice,
            ManualReflectionNotice {
                text: expected_text.to_string(),
                is_error: expected_is_error,
            },
            "终态文案与错误语义必须出自同一策略: {outcome:?}"
        );
        assert_eq!(
            manual_reflection_outcome_text(outcome),
            (notice.text.clone(), notice.is_error),
            "外部 (String, bool) 入口必须是策略结果的投影: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn manual_run_awaits_completion_and_freezes_the_visible_messages() {
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
    let binding = fake_binding();
    let memory: Arc<dyn memory::api::MemoryPort> = Arc::new(memory::api::NoOpMemory);
    let history = crate::application::reflection::test_support::noop_reflection_history();

    let outcome = crate::application::loop_engine::chat::reflection::run(
        &adapter,
        crate::application::reflection::ReflectionTaskTrigger::Manual,
        &enabled_memory_config(),
        vec![Message::user("visible history")],
        &binding,
        "system",
        "zh",
        &memory,
        &history,
        None,
        CancellationToken::new(),
    )
    .await;

    let ReflectionRunOutcome::Completed(completion) = outcome else {
        panic!("an enabled configuration must not skip the manual run");
    };
    assert_eq!(
        completion.trigger,
        crate::application::reflection::ReflectionTaskTrigger::Manual
    );
    assert_eq!(completion.status, ReflectionTaskCompletionStatus::Succeeded);
    assert_eq!(
        completion
            .metadata
            .as_ref()
            .map(|metadata| metadata.applied_changes()),
        Some(0),
        "auto-apply is off, so a completed run must not claim a change",
    );
}

// ---------------------------------------------------------------------------
// Manual Run：ChatManualReflection 生产端口（快照传递 + outcome 映射 + 文案）
// ---------------------------------------------------------------------------

use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

use crate::application::loop_engine::chat::main_run_port::ChatManualReflection;
use crate::application::loop_engine::{ManualReflectionOutcome, ManualReflectionPort};
use crate::application::run::run_factory_support::SessionRunFixture;
use crate::domain::agent_run::RunSpec;
use share::config::domain::snapshot::ConfigSnapshot;

/// 记录反思 prompt 的 Provider：`execute_reflection` 把冻结的会话快照经
/// `build_prompt` 汇入单条 user prompt，因此 prompt 文本是快照到达执行层的直接证据。
#[derive(Clone)]
struct RecordingReflectionProvider {
    prompts: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl crate::ports::ProviderPort for RecordingReflectionProvider {
    // `capabilities()` 已删除（#1880）：binding 持全量 ModelInfo，运行时零查询。

    async fn invoke(
        &self,
        request: crate::ports::provider_port::InvocationRequestData,
        _cancel: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::ProviderResponseStream,
        crate::ports::provider_port::ProviderError,
    > {
        self.prompts.lock().unwrap().push(
            request
                .messages
                .iter()
                .map(|message| message.text_content())
                .collect::<Vec<_>>()
                .join("\n"),
        );
        Ok(
            crate::application::model::test_support::text_completion_stream(
                r#"{"deviations":[],"suggested_memories":[],"outdated_memories":[]}"#,
                1,
                1,
            ),
        )
    }
}

fn recording_binding(prompts: Arc<Mutex<Vec<String>>>) -> Arc<crate::ports::ProviderBindingData> {
    Arc::new(crate::ports::ProviderBindingData {
        provider: Arc::new(RecordingReflectionProvider { prompts }),
        model: provider::ModelInfo {
            provider: "manual-reflect-test".to_string(),
            model: "manual-test-model".to_string(),
            supports_tools: false,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            reasoning: provider::ReasoningCapabilityData::none(),
            context_limit: Some(128_000),
            output_limit: Some(8_192),
        },
        max_tokens: 8_192,
        requested_reasoning: share::reasoning::ReasoningLevel::Off,
    })
}

/// 生产装配形态的 `ChatManualReflection`：runtime context + 反思 adapter +
/// system prompt + language + 冻结的 committed 快照。usage sink 由调用方注入
/// 以断言 Usage 记账。
fn manual_reflection_port(
    config: share::config::Config,
    prompts: Arc<Mutex<Vec<String>>>,
    usage_sink: Arc<
        crate::application::loop_engine::chat::pre_compact_trigger_tests::RecordingUsageSink,
    >,
) -> (SessionRunFixture, ChatManualReflection) {
    let fixture = SessionRunFixture::builder()
        .with_config(ConfigSnapshot::new(config))
        .with_provider_binding(recording_binding(prompts))
        .with_reflection_history(
            crate::application::reflection::test_support::noop_reflection_history(),
        )
        .with_session_id("manual-reflection".to_string())
        .with_usage_sink(usage_sink)
        .build();
    let runtime_context = fixture
        .create(RunSpec::manual_reflection())
        .expect("create manual reflection run")
        .context()
        .clone();
    let port = ChatManualReflection {
        runtime_context,
        reflection_tasks: ReflectionTaskAdapter::production(Duration::from_secs(5)),
        system_prompt: "system".to_string(),
        language: "zh".to_string(),
        messages: vec![Message::user("visible committed history")],
        coverage_end: None,
    };
    (fixture, port)
}

/// 端口把装配前冻结的 committed 快照原样交给反思执行（prompt 必须携带快照文本），
/// Succeeded 映射为 `Ready(Succeeded)`，并按 `manual_reflection_outcome_text`
/// 发布终态 CommandResultText。
#[tokio::test]
async fn chat_manual_reflection_runs_frozen_snapshot_and_publishes_success_text() {
    let prompts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let (fixture, mut port) = manual_reflection_port(
        share::config::Config::default(),
        Arc::clone(&prompts),
        Arc::new(
            crate::application::loop_engine::chat::pre_compact_trigger_tests::RecordingUsageSink::default(),
        ),
    );

    let outcome = port
        .run_manual_reflection(&sdk::RunId::new_v7(), &CancellationToken::new())
        .await
        .expect("端口执行不得返回 Err");

    assert_eq!(
        outcome,
        ManualReflectionOutcome::Ready(ReflectionTaskCompletionStatus::Succeeded)
    );
    assert!(
        prompts
            .lock()
            .unwrap()
            .iter()
            .any(|prompt| prompt.contains("visible committed history")),
        "冻结的 committed 快照必须到达反思 prompt: {:?}",
        prompts.lock().unwrap()
    );
    let texts = fixture.event_sink().command_result_texts();
    assert!(
        texts
            .iter()
            .any(|(text, is_error)| text.contains("已完成") && !is_error),
        "Succeeded 必须发布成功文案且非错误: {texts:?}"
    );
}

/// 取消折叠与禁用竞态的映射：预取消 token → `Cancelled`；受理后配置被关闭 →
/// `DisabledSkipped` 折叠为 `Ready(Failed)` 并发布「未启用」文案（Run 照常收口）。
#[tokio::test]
async fn chat_manual_reflection_maps_cancelled_and_disabled_outcomes() {
    // ① 预先取消的 token：run_complete 折叠为 Completed(Cancelled) → 端口映射 Cancelled。
    let cancelled_sink = Arc::new(
        crate::application::loop_engine::chat::pre_compact_trigger_tests::RecordingUsageSink::default(),
    );
    let (_fixture, mut port) = manual_reflection_port(
        share::config::Config::default(),
        {
            let prompts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            prompts
        },
        cancelled_sink.clone(),
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = port
        .run_manual_reflection(&sdk::RunId::new_v7(), &cancel)
        .await
        .expect("取消不是错误");
    assert_eq!(outcome, ManualReflectionOutcome::Cancelled);
    assert!(
        cancelled_sink.records().is_empty(),
        "Cancelled 手动反思不得计入 Usage: {:?}",
        cancelled_sink.records()
    );

    // ② 禁用竞态：run_complete 返回 DisabledSkipped → Ready(Failed) + 未启用文案。
    let disabled = share::config::Config {
        memory: share::config::MemoryConfig {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let disabled_sink = Arc::new(
        crate::application::loop_engine::chat::pre_compact_trigger_tests::RecordingUsageSink::default(),
    );
    let (fixture, mut port) = manual_reflection_port(
        disabled,
        {
            let prompts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            prompts
        },
        disabled_sink.clone(),
    );
    let outcome = port
        .run_manual_reflection(&sdk::RunId::new_v7(), &CancellationToken::new())
        .await
        .expect("禁用竞态不是错误");
    assert_eq!(
        outcome,
        ManualReflectionOutcome::Ready(ReflectionTaskCompletionStatus::Failed)
    );
    assert!(
        disabled_sink.records().is_empty(),
        "DisabledSkipped 手动反思不得计入 Usage: {:?}",
        disabled_sink.records()
    );
    let texts = fixture.event_sink().command_result_texts();
    assert!(
        texts
            .iter()
            .any(|(text, is_error)| text.contains("未启用") && !is_error),
        "禁用竞态必须发布未启用文案且非错误: {texts:?}"
    );
}

/// Manual Reflection Run 无 RunStep——成功终态经共享
/// `record_successful_usage` 仍恰好记 1 条，`run_step_id`/`model_invocation_id`
/// 用仅记账的 UUIDv7，run/session/model identity 与 provider 报告的 tokens 正确。
#[tokio::test]
async fn chat_manual_reflection_success_records_usage_without_run_step() {
    let usage_sink = Arc::new(
        crate::application::loop_engine::chat::pre_compact_trigger_tests::RecordingUsageSink::default(),
    );
    let (_fixture, mut port) = manual_reflection_port(
        share::config::Config::default(),
        Arc::new(Mutex::new(Vec::new())),
        usage_sink.clone(),
    );
    let run_id = sdk::RunId::new("manual-usage-run");

    let outcome = port
        .run_manual_reflection(&run_id, &CancellationToken::new())
        .await
        .expect("端口执行不得返回 Err");
    assert_eq!(
        outcome,
        ManualReflectionOutcome::Ready(ReflectionTaskCompletionStatus::Succeeded)
    );

    let records = usage_sink.records();
    assert_eq!(
        records.len(),
        1,
        "成功 Manual 反思必须恰好记 1 条 UsageRecord: {records:?}"
    );
    let record = &records[0];
    assert_eq!(record.run_id, run_id, "run identity 必须是手动反思 Run");
    assert_eq!(
        record.session_id,
        sdk::SessionId::new(port.runtime_context.skill_load_session_id())
    );
    assert_eq!(record.provider, "manual-reflect-test");
    assert_eq!(record.model, "manual-test-model");
    assert_eq!(
        record.input_tokens, 1,
        "provider 报告的 input tokens 原样入账"
    );
    assert_eq!(
        record.output_tokens, 1,
        "provider 报告的 output tokens 原样入账"
    );
    assert_eq!(
        record.run_step_id.as_uuid().get_version_num(),
        7,
        "Manual Run 无 RunStep：run_step_id 必须是仅记账的 UUIDv7"
    );
    assert_eq!(
        record.model_invocation_id.as_uuid().get_version_num(),
        7,
        "model_invocation_id 必须是仅记账的 UUIDv7"
    );
}
