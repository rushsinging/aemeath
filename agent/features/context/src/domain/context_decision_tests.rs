use std::collections::HashMap;

use provider::ReasoningLevel;
use sdk::{RunId, RunStepId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::Message;

use super::{
    context_decision, ContextRequest, ContextRequestId, DecisionReason, Language, SessionId,
    SystemPromptSpec, TaskReminderSnapshot,
};

fn request(last_api_total_tokens: Option<u64>) -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![Message::user("pending")],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".to_string(),
        effective_reasoning: ReasoningLevel::Off,
        task_reminder: TaskReminderSnapshot::default(),
        language: Language::new("zh"),
        agent_roles: HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 1_000,
        max_output_tokens: 100,
        last_api_total_tokens,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

#[test]
fn provider_total_is_used_without_projected_delta() {
    let decision = context_decision::calculate(
        &request(Some(700)),
        &[Message::user("x".repeat(4_000))],
        &[],
        None,
    );

    assert_eq!(decision.decision_token_count, 700);
    assert!(!decision.needed);
    assert_eq!(decision.reason, DecisionReason::ActualProviderUsage);
}

#[test]
fn provider_total_above_threshold_triggers_compaction() {
    let decision = context_decision::calculate(&request(Some(900)), &[], &[], None);

    assert!(decision.needed);
    assert_eq!(decision.reason, DecisionReason::ActualProviderUsage);
}

#[test]
fn missing_provider_total_falls_back_to_complete_candidate_estimate() {
    let decision = context_decision::calculate(
        &request(None),
        &[Message::user("x".repeat(4_000))],
        &[],
        None,
    );

    assert!(decision.needed);
    assert!(decision.decision_token_count > decision.threshold);
    assert_eq!(decision.reason, DecisionReason::HeuristicFallback);
}
