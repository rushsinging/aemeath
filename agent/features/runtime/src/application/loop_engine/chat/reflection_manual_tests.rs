//! External tests for the Manual reflection trigger (#1289).
//!
//! `/reflect-now` 在 idle 路径同步等待反思完成后回显结果文案：只有配置禁用
//! 是显式跳过，执行失败按 `is_error` 上报，其余为正常完成。

use std::sync::Arc;
use std::time::Duration;

use crate::application::loop_engine::chat::reflection::{
    manual_reflection_outcome_text, run_manual_reflection,
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
        provider: Arc::new(crate::application::loop_engine::chat::pre_compact_trigger_tests::StaticReflectionProvider),
        model: provider::ModelIdData {
            // `StaticReflectionProvider` only answers for this provider name.
            provider: "pre-compact-test".to_string(),
            model: "manual-test-model".to_string(),
        },
        max_tokens: 8_192,
        requested_reasoning: share::reasoning::ReasoningLevel::Off,
        context_window: Some(128_000),
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
            duration_ms: 1,
            record_id: None,
        }),
    }
}

#[test]
fn manual_outcome_text_reports_disabled_without_error() {
    let (text, is_error) = manual_reflection_outcome_text(&ReflectionRunOutcome::DisabledSkipped);
    assert!(!is_error);
    assert!(text.contains("未启用"));
}

#[test]
fn manual_outcome_text_reports_the_completed_change_count() {
    let (text, is_error) = manual_reflection_outcome_text(&ReflectionRunOutcome::Completed(
        completed(ReflectionTaskCompletionStatus::Succeeded, 3),
    ));
    assert!(!is_error);
    assert!(text.contains('3'), "the count must reach the user: {text}");
    assert!(text.contains("已完成"));
}

#[test]
fn manual_outcome_text_reports_zero_changes_without_a_count() {
    let (text, is_error) = manual_reflection_outcome_text(&ReflectionRunOutcome::Completed(
        completed(ReflectionTaskCompletionStatus::Succeeded, 0),
    ));
    assert!(!is_error);
    assert!(text.contains("没有记忆变更"), "{text}");
}

#[test]
fn only_a_failed_run_reports_error_semantics() {
    let (text, is_error) = manual_reflection_outcome_text(&ReflectionRunOutcome::Completed(
        completed(ReflectionTaskCompletionStatus::Failed, 0),
    ));
    assert!(is_error);
    assert!(text.contains("失败"), "{text}");

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

#[tokio::test]
async fn manual_run_awaits_completion_and_freezes_the_visible_messages() {
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
    let binding = fake_binding();
    let memory: Arc<dyn memory::api::MemoryPort> = Arc::new(memory::api::NoOpMemory);
    let history = crate::application::reflection::test_support::noop_reflection_history();

    let outcome = run_manual_reflection(
        &adapter,
        &enabled_memory_config(),
        &[Message::user("visible history")],
        &binding,
        "system",
        "zh",
        &memory,
        &history,
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
