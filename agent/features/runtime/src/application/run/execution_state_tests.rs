use super::execution_state::RunExecutionState;
use crate::application::loop_engine::PendingInteractionWork;
use crate::ports::{
    CompactionDecision, ContextRequest, ContextRequestId, ContextWindow, DecisionReason, Language,
    SessionId, SessionRevision, SystemPromptSpec, TokenBudget, Urgency,
};
use provider::ReasoningLevel;
use sdk::{RunId, RunStepId};
use share::{
    config::{domain::snapshot::ConfigSnapshot, Config},
    message::Message,
};
use std::collections::HashMap;

fn context_request(step_id: &str) -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new(format!("request-{step_id}")),
        run_id: RunId::new("run"),
        step_id: RunStepId::new(step_id),
        pending_messages: vec![],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".to_string(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::new("zh"),
        agent_roles: HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

fn context_window(revision: u64) -> ContextWindow {
    ContextWindow {
        backing_revision: SessionRevision::new(revision),
        system_blocks: vec![],
        messages: vec![].into(),
        tool_schemas: vec![],
        token_estimation: TokenBudget::default(),
        compaction_decision: CompactionDecision {
            needed: false,
            urgency: Urgency::None,
            decision_token_count: 0,
            threshold: 0,
            context_size: 200_000,
            effective_window: 180_000,
            reason: DecisionReason::HeuristicFallback,
        },
    }
}

#[test]
fn new_execution_state_starts_with_empty_working_data() {
    let state = RunExecutionState::new();

    assert!(state.messages().is_empty());
    assert!(state.accepted_input().is_empty());
    assert!(state.context_request().is_none());
    assert!(state.context_window().is_none());
    assert_eq!(state.step_count(), 0);
    assert!(state.pending_interaction_work().is_none());
}

#[test]
fn execution_observation_state_tracks_start_and_terminal_once() {
    let mut state = RunExecutionState::new();
    assert!(state.started_at().is_none());

    state.initialize_for_launch(Vec::new(), 0);
    assert!(state.started_at().is_some());
    state.set_terminal(tools::AgentRunTerminal::Cancelled);
    assert_eq!(
        state.take_terminal(),
        Some(tools::AgentRunTerminal::Cancelled)
    );
    assert!(state.take_terminal().is_none());
}

#[test]
fn adopted_input_is_replaced_and_taken_once() {
    let mut state = RunExecutionState::new();
    state.replace_adopted_input(vec![(
        sdk::InputId::new("input-1"),
        Message::user("queued"),
    )]);

    assert_eq!(state.adopted_input().len(), 1);
    assert_eq!(state.take_adopted_input().len(), 1);
    assert!(state.take_adopted_input().is_empty());
}

#[test]
fn pending_interaction_work_has_single_take_owner() {
    let mut state = RunExecutionState::new();
    state.set_pending_interaction_work(PendingInteractionWork::default());

    assert!(state.pending_interaction_work().is_some());
    assert!(state.take_pending_interaction_work().is_some());
    assert!(state.take_pending_interaction_work().is_none());
}

#[test]
fn with_step_count_preserves_bootstrap_turn() {
    let mut state = RunExecutionState::new();
    state.initialize_for_launch(Vec::new(), 4);

    assert_eq!(state.step_count(), 4);
    assert_eq!(state.advance_step(), 5);
}

#[test]
fn launch_initialization_sets_messages_turn_and_start_once() {
    let mut state = RunExecutionState::new();
    state.initialize_for_launch(vec![Message::user("hello")], 4);

    assert_eq!(state.messages().len(), 1);
    assert_eq!(state.step_count(), 4);
    assert!(state.elapsed() >= std::time::Duration::ZERO);
}

#[test]
fn step_count_is_owned_by_execution_state() {
    let mut state = RunExecutionState::new();

    assert_eq!(state.advance_step(), 1);
    assert_eq!(state.advance_step(), 2);
    assert_eq!(state.step_count(), 2);
}

#[test]
fn context_projection_is_replaced_as_one_step_snapshot() {
    let mut state = RunExecutionState::new();
    state.replace_context_state(context_request("step-1"), None);

    assert_eq!(
        state.context_request().unwrap().request_id,
        ContextRequestId::new("request-step-1"),
    );
    assert!(state.context_window().is_none());

    *state.context_window_mut() = Some(context_window(7));
    assert_eq!(
        state.context_window().unwrap().backing_revision,
        SessionRevision::new(7)
    );

    state.replace_context_state(context_request("step-2"), None);
    assert_eq!(
        state.context_request().unwrap().request_id,
        ContextRequestId::new("request-step-2"),
    );
    assert!(state.context_window().is_none());
}

#[test]
fn pending_step_messages_are_consumed_only_when_no_explicit_input_exists() {
    let mut state = RunExecutionState::new();
    state.replace_pending_step_messages(vec![Message::user("pending")]);

    let first = state.freeze_step_input_messages(None, vec![Message::user("explicit")]);
    assert_eq!(first.len(), 1);
    assert_eq!(state.pending_step_messages_len(), 1);

    let second = state.freeze_step_input_messages(None, vec![]);
    assert_eq!(second.len(), 1);
    assert!(state.pending_step_messages_len() == 0);
    assert_eq!(state.accepted_input().len(), 1);
}

#[test]
fn step_messages_are_recorded_and_committed_by_execution_state() {
    let mut state = RunExecutionState::new();
    state.freeze_step_messages(vec![Message::user("input")]);
    state.record_step_message(Message {
        role: share::message::Role::Assistant,
        content: vec![],
        metadata: None,
    });

    assert_eq!(state.step_outcome().len(), 1);
    assert_eq!(state.active_step_messages_len(), 2);

    state.commit_step_messages();
    assert!(state.step_outcome().is_empty());
    assert!(state.active_step_messages_len() == 0);
}

#[test]
fn messages_and_accepted_input_are_run_owned_working_sets() {
    let mut state = RunExecutionState::new();
    state.append_message(Message::user("history"));
    state.replace_accepted_input(vec![Message::user("accepted")]);

    assert_eq!(state.messages_snapshot().len(), 1);
    assert_eq!(state.accepted_input_snapshot().len(), 1);

    state.clear_accepted_input();
    assert!(state.accepted_input().is_empty());
}

#[test]
fn begin_step_starts_step_elapsed_clock() {
    let mut state = RunExecutionState::new();
    assert_eq!(state.step_elapsed(), None);

    state.begin_step();

    assert!(state.step_elapsed().is_some());
}

#[test]
fn begin_step_replaces_transient_step_data_without_discarding_messages() {
    let mut state = RunExecutionState::new();
    state.append_message(Message::user("history"));
    state.replace_accepted_input(vec![Message::user("accepted")]);
    state.advance_step();

    state.begin_step();

    assert_eq!(state.messages().len(), 1);
    assert!(state.accepted_input().is_empty());
    assert!(state.context_request().is_none());
    assert!(state.context_window().is_none());
    assert_eq!(state.step_count(), 1);
}
