use super::*;
use crate::application::agent::ToolCall;
use sdk::ChatInputEvent;
use serde_json::json;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::domain::agent_run::{Run, RunDomainEvent, RunSpec, RunStatus};

#[derive(Default)]
struct ScriptedPort {
    model_steps: VecDeque<ModelStep>,
    tool_steps: VecDeque<ToolStep>,
    calls: Vec<&'static str>,
    events: Vec<RunDomainEvent>,
    guarded_calls: Vec<Vec<ToolGuardDecision>>,
    input_batches: VecDeque<Vec<LoopInput>>,
    cancelled_during_model: bool,
    block_model_forever: bool,
    block_compact_until_cancelled: bool,
    needs_compaction: bool,
    fail_emit_once: bool,
}

#[async_trait::async_trait]
impl RunLoopPort for ScriptedPort {
    async fn drain_input(&mut self) -> Result<Vec<LoopInput>, LoopEngineError> {
        self.calls.push("input");
        Ok(self.input_batches.pop_front().unwrap_or_default())
    }

    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        self.calls.push("needs_compaction");
        Ok(self.needs_compaction)
    }

    async fn compact(&mut self, cancel: &CancellationToken) -> Result<(), LoopEngineError> {
        self.calls.push("compact");
        if self.block_compact_until_cancelled {
            cancel.cancelled().await;
            return Err(LoopEngineError::Cancelled);
        }
        Ok(())
    }

    async fn invoke_model(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
        self.calls.push("model");
        if self.block_model_forever {
            std::future::pending::<()>().await;
        }
        if self.cancelled_during_model {
            cancel.cancelled().await;
            return Err(LoopEngineError::Cancelled);
        }
        self.model_steps
            .pop_front()
            .map(|step| (step, StepTokenUsage::default()))
            .ok_or_else(|| LoopEngineError::Adapter("missing model step".to_string()))
    }

    async fn execute_tools(
        &mut self,
        calls: &[(ToolCall, ToolGuardDecision)],
        _cancel: &CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        self.calls.push("tools");
        self.guarded_calls
            .push(calls.iter().map(|(_, decision)| decision.clone()).collect());
        self.tool_steps
            .pop_front()
            .ok_or_else(|| LoopEngineError::Adapter("missing tool step".to_string()))
    }

    async fn on_stuck(&mut self, _decision: &StuckDecision) -> Result<(), LoopEngineError> {
        self.calls.push("stuck");
        Ok(())
    }

    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        self.calls.push("emit");
        if self.fail_emit_once {
            self.fail_emit_once = false;
            return Err(LoopEngineError::Adapter("sink failed".to_string()));
        }
        self.events.extend(events);
        Ok(())
    }
}

fn new_run(timeout: Duration) -> Run {
    Run::new(RunSpec::new("test", timeout), None)
}

fn call(name: &str, input: serde_json::Value) -> ToolCall {
    ToolCall {
        id: sdk::ToolCallId::new_v7(),
        provider_id: format!("provider-{name}"),
        name: name.to_string(),
        index: 0,
        input,
    }
}

#[test]
fn input_split_keeps_user_content_and_controls_separate() {
    let batch = split_input_events(vec![
        ChatInputEvent::user_message("follow up", Vec::new()),
        ChatInputEvent::ControlCommand {
            raw: "/save".to_string(),
        },
        ChatInputEvent::Reset,
    ]);

    assert_eq!(batch.user_inputs.len(), 1);
    assert_eq!(batch.user_inputs[0].text, "follow up");
    assert_eq!(
        batch.controls,
        vec![
            RuntimeControl::Command("/save".to_string()),
            RuntimeControl::Reset,
        ]
    );
}

#[test]
fn stuck_guard_detects_repeated_text_for_every_run_kind() {
    for mut guard in [
        StuckGuard::new(Duration::ZERO, 2),
        StuckGuard::new(Duration::from_secs(30), 2),
    ] {
        assert_eq!(guard.inspect_text("same"), StuckDecision::Allow);
        assert_eq!(guard.inspect_text("same"), StuckDecision::Allow);
        assert!(matches!(
            guard.inspect_text("same"),
            StuckDecision::SoftBlock { .. }
        ));
    }
}

#[test]
fn stuck_guard_detects_tool_loops_and_escalates() {
    let mut guard = StuckGuard::new(Duration::ZERO, 2);
    let repeated = call("Read", json!({"file_path": "a.rs"}));

    assert_eq!(guard.inspect_tool(&repeated), StuckDecision::Allow);
    assert_eq!(guard.inspect_tool(&repeated), StuckDecision::Allow);
    assert!(matches!(
        guard.inspect_tool(&repeated),
        StuckDecision::SoftBlock { .. }
    ));
    let _ = guard.inspect_tool(&repeated);
    assert!(matches!(
        guard.inspect_tool(&repeated),
        StuckDecision::HardPause { .. }
    ));
}

#[test]
fn timeout_zero_is_unlimited_and_positive_timeout_fails() {
    let now = Instant::now();
    let unlimited = StuckGuard::with_started_at(Duration::ZERO, 2, now);
    let finite = StuckGuard::with_started_at(Duration::from_secs(5), 2, now);

    assert_eq!(
        unlimited.inspect_timeout(now + Duration::from_secs(60)),
        StuckDecision::Allow
    );
    assert!(matches!(
        finite.inspect_timeout(now + Duration::from_secs(5)),
        StuckDecision::Fail { .. }
    ));
}

#[test]
fn stop_hook_limit_fails_instead_of_looping_forever() {
    let mut guard = StuckGuard::new(Duration::ZERO, 2);

    assert!(matches!(
        guard.record_stop_hook_block(),
        StuckDecision::SoftBlock { .. }
    ));
    assert!(matches!(
        guard.record_stop_hook_block(),
        StuckDecision::Fail { .. }
    ));
}

#[tokio::test]
async fn engine_completes_text_only_run_through_the_run_fsm() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(run.steps().len(), 1);
    assert_eq!(
        run.steps()[0].invocation().unwrap().response(),
        "done",
        "the shared engine must record the model invocation in the Run aggregate"
    );
    assert_eq!(
        port.calls,
        vec!["emit", "input", "needs_compaction", "emit", "model", "emit"]
    );
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Completed { .. })));
}

#[tokio::test]
async fn engine_executes_tools_then_reenters_the_same_loop() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([
            ModelStep::Tools {
                text: "calling".to_string(),
                calls: vec![call("Read", json!({"file_path": "a.rs"}))],
            },
            ModelStep::Complete {
                text: "done".to_string(),
            },
        ]),
        tool_steps: VecDeque::from([ToolStep::Continue]),
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(
        port.calls.iter().filter(|call| **call == "model").count(),
        2
    );
    assert_eq!(
        port.calls.iter().filter(|call| **call == "tools").count(),
        1
    );
    let first_step = &run.steps()[0];
    assert_eq!(first_step.tool_calls().len(), 1);
    assert_eq!(
        first_step.tool_calls()[0].status(),
        crate::domain::agent_run::ToolCallStatus::Success,
        "the shared engine must own the tool-call lifecycle"
    );
}

#[tokio::test]
async fn engine_pauses_for_user_without_completing_the_run() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);
}

#[tokio::test]
async fn engine_cancels_in_flight_compaction_and_emits_terminal_ack() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        ..Default::default()
    };
    let cancel_for_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::task::yield_now().await;
        cancel_for_task.cancel();
    });

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();
    canceller.await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Cancelled);
    assert!(port.calls.contains(&"compact"));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(!port.calls.contains(&"model"));
}

#[tokio::test]
async fn engine_cancels_in_flight_model_and_emits_terminal_ack() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        cancelled_during_model: true,
        ..Default::default()
    };
    let cancel_for_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::task::yield_now().await;
        cancel_for_task.cancel();
    });

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();
    canceller.await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Cancelled);
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::CancellationRequested { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
}

#[tokio::test]
async fn engine_passes_soft_block_decision_to_the_single_tool_adapter() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let repeated = call("Read", json!({"file_path": "a.rs"}));
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([
            ModelStep::Tools {
                text: "one".to_string(),
                calls: vec![repeated.clone()],
            },
            ModelStep::Tools {
                text: "two".to_string(),
                calls: vec![repeated.clone()],
            },
            ModelStep::Tools {
                text: "three".to_string(),
                calls: vec![repeated],
            },
            ModelStep::Complete {
                text: "done".to_string(),
            },
        ]),
        tool_steps: VecDeque::from([ToolStep::Continue, ToolStep::Continue, ToolStep::Continue]),
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(port.guarded_calls.len(), 3);
    assert_eq!(port.guarded_calls[0], vec![ToolGuardDecision::Allow]);
    assert_eq!(port.guarded_calls[1], vec![ToolGuardDecision::Allow]);
    assert!(matches!(
        port.guarded_calls[2].as_slice(),
        [ToolGuardDecision::SoftBlock { .. }]
    ));
}

#[tokio::test]
async fn engine_timeout_interrupts_a_blocked_model_call() {
    let mut run = new_run(Duration::from_millis(10));
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        block_model_forever: true,
        ..Default::default()
    };

    tokio::time::timeout(
        Duration::from_secs(1),
        run_loop(&mut run, &cancel, &mut port),
    )
    .await
    .expect("deadline must interrupt blocked model")
    .unwrap();

    assert_eq!(run.status(), RunStatus::Failed);
}

#[tokio::test]
async fn awaiting_user_does_not_resume_without_input() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };
    assert_eq!(
        run_loop(&mut run, &cancel, &mut port).await.unwrap(),
        LoopDirective::AwaitUser
    );
    let model_calls = port.calls.iter().filter(|call| **call == "model").count();

    assert_eq!(
        run_loop(&mut run, &cancel, &mut port).await.unwrap(),
        LoopDirective::AwaitUser
    );
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert_eq!(
        port.calls.iter().filter(|call| **call == "model").count(),
        model_calls
    );
}

#[tokio::test]
async fn failed_event_delivery_is_restored_to_the_run_outbox() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        fail_emit_once: true,
        ..Default::default()
    };

    let error = run_loop(&mut run, &cancel, &mut port).await.unwrap_err();

    assert!(matches!(error, LoopEngineError::Adapter(_)));
    assert!(matches!(run.events(), [RunDomainEvent::Started { .. }]));
}

#[tokio::test]
async fn engine_timeout_fails_before_starting_new_work() {
    let mut run = new_run(Duration::from_nanos(1));
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort::default();

    tokio::time::sleep(Duration::from_millis(1)).await;
    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Failed);
    assert!(!port.calls.contains(&"model"));
}
