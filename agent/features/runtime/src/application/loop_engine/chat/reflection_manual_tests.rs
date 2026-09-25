//! External tests for the Manual reflection trigger (#1289).
//!
//! `/reflect-now` 在 idle 受理后冻结 committed session 的可见消息快照并提交
//! 共享单槽；受理结果只映射为安全提示文案（无错误分支——busy/disabled 都是
//! 显式跳过）。单槽争用契约本身由 reflection runner 的 task adapter 测试覆盖。

use std::sync::Arc;
use std::time::Duration;

use crate::application::loop_engine::chat::reflection::{
    manual_reflection_outcome_text, submit_manual_reflection,
};
use crate::application::reflection::{ReflectionTaskAdapter, ReflectionTaskSubmitOutcome};
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

fn fake_binding() -> Arc<crate::ports::ProviderBinding> {
    Arc::new(crate::ports::ProviderBinding {
        provider: Arc::new(crate::application::loop_engine::chat::pre_compact_trigger_tests::StaticReflectionProvider),
        model: provider::ModelId {
            provider: "manual-test".to_string(),
            model: "manual-test-model".to_string(),
        },
        max_tokens: 8_192,
        requested_reasoning: provider::ReasoningLevel::Off,
        context_window: Some(128_000),
    })
}

#[test]
fn manual_outcome_text_never_reports_error_semantics() {
    let accepted = manual_reflection_outcome_text(ReflectionTaskSubmitOutcome::Accepted);
    assert!(!accepted.1);
    assert!(accepted.0.contains("已开始"));

    let busy = manual_reflection_outcome_text(ReflectionTaskSubmitOutcome::BusySkipped);
    assert!(!busy.1);
    assert!(busy.0.contains("正在运行"));

    let disabled = manual_reflection_outcome_text(ReflectionTaskSubmitOutcome::DisabledSkipped);
    assert!(!disabled.1);
    assert!(disabled.0.contains("未启用"));
}

#[tokio::test]
async fn manual_submission_freezes_visible_messages_into_shared_slot() {
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
    let binding = fake_binding();
    let memory: Arc<dyn memory::MemoryPort> = Arc::new(memory::NoOpMemory);
    let history = super::pre_compact_trigger_tests::noop_reflection_history();

    let outcome = submit_manual_reflection(
        &adapter,
        &enabled_memory_config(),
        &[Message::user("visible history")],
        &binding,
        "system",
        "zh",
        &memory,
        &history,
    );

    assert_eq!(outcome, ReflectionTaskSubmitOutcome::Accepted);
    // 第二次提交共享单槽：busy skip，不排队。
    let second = submit_manual_reflection(
        &adapter,
        &enabled_memory_config(),
        &[Message::user("more")],
        &binding,
        "system",
        "zh",
        &memory,
        &history,
    );
    assert_eq!(second, ReflectionTaskSubmitOutcome::BusySkipped);
    adapter.drain().await;
}
