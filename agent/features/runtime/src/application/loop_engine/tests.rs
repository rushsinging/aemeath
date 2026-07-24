use super::*;
use crate::application::subagent::ToolCall;
use sdk::ChatInputEvent;
use serde_json::json;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::application::loop_engine::engine::{
    DrainEpoch, DrainOutcome, InternalContinuationKind, LoopInput,
};
use crate::domain::agent_run::{Run, RunControl, RunDomainEvent, RunSpec, RunStatus};

struct ScriptedPort {
    model_steps: VecDeque<ModelStep>,
    model_errors: VecDeque<LoopEngineError>,
    tool_steps: VecDeque<ToolStep>,
    controls: std::sync::Mutex<VecDeque<RunControl>>,
    registered_step: std::sync::Mutex<Option<sdk::RunStepId>>,
    cancel_when_compact_starts: bool,
    cancel_when_model_starts: bool,
    cancel_when_tools_starts: bool,
    terminate_when_compact_starts: bool,
    step_cancel: std::sync::Arc<std::sync::Mutex<Option<CancellationToken>>>,
    calls: Vec<&'static str>,
    events: Vec<RunDomainEvent>,
    guarded_calls: Vec<Vec<ToolGuardDecision>>,
    drain_outcomes: VecDeque<DrainOutcome>,
    /// #1272: ScriptedPort tracks its own drain epoch for validation.
    /// On each `drain_input` call, validates the engine's expected_epoch
    /// against this counter and advances it after a successful drain.
    drain_epoch: DrainEpoch,
    cancelled_during_model: bool,
    block_model_forever: bool,
    block_compact_until_cancelled: bool,
    cancelled_steps: Vec<sdk::RunStepId>,
    finalized_steps: Vec<sdk::RunStepId>,
    frozen_steps: Vec<sdk::RunStepId>,
    fail_accept_input: bool,
    needs_compaction: bool,
    fail_emit_once: bool,
}

impl Default for ScriptedPort {
    fn default() -> Self {
        let mut drain_outcomes = VecDeque::new();
        drain_outcomes.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "test-input".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(0),
        ));
        drain_outcomes.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });
        Self {
            model_steps: Default::default(),
            model_errors: Default::default(),
            tool_steps: Default::default(),
            controls: Default::default(),
            registered_step: Default::default(),
            cancel_when_compact_starts: false,
            cancel_when_model_starts: false,
            cancel_when_tools_starts: false,
            terminate_when_compact_starts: false,
            step_cancel: Default::default(),
            calls: Default::default(),
            events: Default::default(),
            guarded_calls: Default::default(),
            drain_outcomes,
            drain_epoch: DrainEpoch(0),
            cancelled_during_model: false,
            block_model_forever: false,
            block_compact_until_cancelled: false,
            cancelled_steps: Default::default(),
            finalized_steps: Default::default(),
            frozen_steps: Default::default(),
            fail_accept_input: false,
            needs_compaction: false,
            fail_emit_once: false,
        }
    }
}

#[async_trait::async_trait]
impl RunLoopPort for ScriptedPort {
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.calls.push("input");
        // #1272: validate the engine's expected epoch against the
        // port's own tracked epoch before consuming any outcome.
        if expected_epoch != self.drain_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch, self.drain_epoch,
            )));
        }
        let outcome =
            self.drain_outcomes
                .pop_front()
                .unwrap_or_else(|| DrainOutcome::EmptyAndSealed {
                    epoch: self.drain_epoch,
                });
        // #1272: Only advance epoch for outcomes that consumed input.
        // NoInput means no input was consumed — epoch stays the same.
        match &outcome {
            DrainOutcome::NoInput { .. } => {}
            _ => {
                self.drain_epoch = self.drain_epoch.next();
            }
        }
        Ok(outcome)
    }

    /// #1272: For ScriptedPort, await_user_input does NOT delegate to
    /// drain_input because the epoch advancement rules differ:
    /// - drain_input always advances epoch
    /// - await_user_input advances for Ready and InternalContinuation
    ///   but NOT for EmptyAndSealed or NoInput (the engine won't either)
    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.calls.push("await_input");
        if expected_epoch != self.drain_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch, self.drain_epoch,
            )));
        }
        let outcome = self
            .drain_outcomes
            .pop_front()
            .unwrap_or_else(|| DrainOutcome::NoInput {
                epoch: self.drain_epoch,
            });
        // #1272: Don't advance epoch for outcomes that the engine won't advance for
        match &outcome {
            DrainOutcome::EmptyAndSealed { .. } | DrainOutcome::NoInput { .. } => {}
            _ => {
                self.drain_epoch = self.drain_epoch.next();
            }
        }
        Ok(outcome)
    }

    fn freeze_step(&mut self, step_id: &sdk::RunStepId, _inputs: &[LoopInput]) {
        self.calls.push("freeze_step");
        self.frozen_steps.push(step_id.clone());
    }

    async fn accept_step_input(
        &mut self,
        _step_id: &sdk::RunStepId,
    ) -> Result<(), LoopEngineError> {
        self.calls.push("accept_step_input");
        if self.fail_accept_input {
            return Err(LoopEngineError::Adapter(
                "accepted input durable write failed".to_string(),
            ));
        }
        Ok(())
    }

    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        self.calls.push("needs_compaction");
        Ok(self.needs_compaction)
    }

    async fn compact(&mut self, cancel: &CancellationToken) -> Result<(), LoopEngineError> {
        self.calls.push("compact");
        if self.cancel_when_compact_starts {
            let step_id = self
                .registered_step
                .lock()
                .unwrap()
                .clone()
                .expect("step scope must be registered before compact");
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::CancelStep {
                    step_id,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_123),
                });
            cancel.cancel();
        }
        if self.terminate_when_compact_starts {
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::Terminate {
                    reason: sdk::RunTerminationReason::UserExit,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_456),
                });
            cancel.cancel();
        }
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
        if self.cancel_when_model_starts {
            let step_id = self
                .registered_step
                .lock()
                .unwrap()
                .clone()
                .expect("step scope must be registered before model");
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::CancelStep {
                    step_id,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_123),
                });
            cancel.cancel();
            return Err(LoopEngineError::Cancelled);
        }
        if self.block_model_forever {
            std::future::pending::<()>().await;
        }
        if self.cancelled_during_model {
            cancel.cancelled().await;
            return Err(LoopEngineError::Cancelled);
        }
        if let Some(error) = self.model_errors.pop_front() {
            return Err(error);
        }
        self.model_steps
            .pop_front()
            .map(|step| (step, StepTokenUsage::default()))
            .ok_or_else(|| LoopEngineError::Adapter("missing model step".to_string()))
    }

    async fn finalize_step(&mut self, step_id: &sdk::RunStepId) -> Result<(), LoopEngineError> {
        self.calls.push("finalize_step");
        self.finalized_steps.push(step_id.clone());
        Ok(())
    }

    async fn finalize_cancelled_step(
        &mut self,
        step_id: &sdk::RunStepId,
    ) -> Result<(), LoopEngineError> {
        self.calls.push("finalize_cancelled_step");
        self.cancelled_steps.push(step_id.clone());
        Ok(())
    }

    async fn execute_tools(
        &mut self,
        _run_id: &sdk::RunId,
        _step_id: &sdk::RunStepId,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        self.calls.push("tools");
        self.guarded_calls
            .push(calls.iter().map(|(_, decision)| decision.clone()).collect());
        if self.cancel_when_tools_starts {
            let step_id = self
                .registered_step
                .lock()
                .unwrap()
                .clone()
                .expect("step scope must be registered before tools");
            self.controls
                .lock()
                .unwrap()
                .push_back(RunControl::CancelStep {
                    step_id,
                    deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_123),
                });
            cancel.cancel();
            return Err(LoopEngineError::Cancelled);
        }
        self.tool_steps
            .pop_front()
            .ok_or_else(|| LoopEngineError::Adapter("missing tool step".to_string()))
    }

    async fn on_stuck(&mut self, _decision: &StuckDecision) -> Result<(), LoopEngineError> {
        self.calls.push("stuck");
        Ok(())
    }

    fn take_control(&self, _run_id: &sdk::RunId) -> Option<RunControl> {
        self.controls.lock().unwrap().pop_front()
    }

    fn register_step_scope(
        &self,
        _run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        *self.registered_step.lock().unwrap() = Some(step_id);
        *self.step_cancel.lock().unwrap() = Some(cancel);
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
    assert_eq!(port.frozen_steps.len(), 1);
    assert_eq!(port.finalized_steps, port.frozen_steps);
    assert_eq!(run.steps()[0].id(), &port.frozen_steps[0]);
    assert_eq!(run.steps().len(), 1);
    assert_eq!(
        run.steps()[0].invocation().unwrap().response(),
        "done",
        "the shared engine must record the model invocation in the Run aggregate"
    );
    assert_eq!(
        port.calls,
        vec![
            "emit",
            "input",
            "freeze_step",
            "accept_step_input",
            "emit",
            "needs_compaction",
            "model",
            "finalize_step",
            "input",
            "emit",
        ]
    );
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Completed { .. })));
}

#[tokio::test]
async fn engine_accepts_input_before_building_context() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    let accepted = port
        .calls
        .iter()
        .position(|call| *call == "accept_step_input")
        .unwrap();
    let context = port
        .calls
        .iter()
        .position(|call| *call == "needs_compaction")
        .unwrap();
    assert!(accepted < context);
}

#[tokio::test]
async fn engine_stops_before_context_when_accepted_input_durable_write_fails() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        fail_accept_input: true,
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Failed);
    assert!(port.calls.contains(&"accept_step_input"));
    assert!(!port.calls.contains(&"needs_compaction"));
    assert!(!port.calls.contains(&"model"));
}

#[tokio::test]
async fn engine_executes_tools_then_reenters_the_same_loop() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "second".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(1),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(2),
            },
        ]),
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
async fn provider_context_too_long_compacts_then_rebuilds_before_reinvoking() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
        }]),
        model_errors: VecDeque::from([LoopEngineError::NeedsCompaction(
            "provider context too long".to_string(),
        )]),
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(
        port.calls,
        vec![
            "emit",
            "input",
            "freeze_step",
            "accept_step_input",
            "emit",
            "needs_compaction",
            "model",
            "compact",
            "model",
            "finalize_step",
            "input",
            "emit",
        ]
    );
}

#[tokio::test]
async fn provider_context_too_long_after_compaction_fails_without_looping() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_errors: VecDeque::from([
            LoopEngineError::NeedsCompaction("first".to_string()),
            LoopEngineError::NeedsCompaction("second".to_string()),
        ]),
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Failed);
    assert_eq!(
        port.calls.iter().filter(|call| **call == "compact").count(),
        1
    );
    assert_eq!(
        port.calls.iter().filter(|call| **call == "model").count(),
        2
    );
}

#[tokio::test]
async fn cancel_step_during_compaction_finalizes_then_returns_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        cancel_when_compact_starts: true,
        ..Default::default()
    };

    run_loop(&mut run, &root, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(port.cancelled_steps, port.frozen_steps);
    assert!(port.calls.contains(&"compact"));
    assert!(!port.calls.contains(&"model"));
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::StepCancelled { .. })));
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
    assert_eq!(port.cancelled_steps, port.frozen_steps);
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
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "one".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "two".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(1),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "three".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(2),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "four".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(3),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(4),
            },
        ]),
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
    assert!(matches!(
        run.events(),
        [
            RunDomainEvent::Transitioned { .. },
            RunDomainEvent::Started { .. },
            RunDomainEvent::DrainingInput { .. }
        ]
    ));
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

// ── #1272 Drain outcome tests ──────────────────────────────────────────

/// InternalContinuation with ToolResults kind processes like user input
/// but uses DrainInternalContinuation transition (not DrainInputs).
#[tokio::test]
async fn engine_processes_internal_continuation() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::InternalContinuation {
                kind: InternalContinuationKind::ToolResults,
                batch: vec![],
                epoch: DrainEpoch(0),
            },
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "resumed".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    // drain_input + freeze + accept + compaction check + emit + model + finalize + emit
    assert!(port.calls.contains(&"freeze_step"));
    assert!(port.calls.contains(&"model"));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Completed { .. })));
}

/// #1272: InternalContinuation with empty batch while AwaitingUser
/// must NOT auto-resume. The engine returns AwaitUser;
/// only Ready (guaranteed non-empty) resumes from AwaitingUser.
#[tokio::test]
async fn internal_continuation_while_awaiting_user_without_input_stays_awaiting() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    // First call: model → Tools → AwaitUser → EmptyAndSealed → AwaitUser
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
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
    let calls_before_second_loop = port.calls.len();

    // Simulate: before user responds, a stop-hook fires.
    // The main adapter would produce InternalContinuation with empty batch.
    // Engine must stay AwaitingUser, not auto-resume.
    // #1272: after the first run_loop consumed Ready(epoch 0), the Run's
    // next_drain_epoch is 1 (EmptyAndSealed during AwaitingUser does NOT
    // advance epoch). InternalContinuation at epoch 1 will advance to 2.
    port.drain_outcomes = VecDeque::from([
        DrainOutcome::InternalContinuation {
            kind: InternalContinuationKind::StopHookFeedback {
                feedback: "stop hook".to_string(),
            },
            batch: vec![], // No user input yet
            epoch: DrainEpoch(1),
        },
        DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(2),
        },
    ]);

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();
    assert_eq!(
        directive,
        LoopDirective::AwaitUser,
        "InternalContinuation with empty batch must NOT resume from AwaitingUser"
    );
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    // Only drain was called (no step processing). When AwaitingUser,
    // the engine calls await_user_input, which pushes "await_input".
    assert_eq!(
        port.calls.len(),
        calls_before_second_loop + 1,
        "Only one drain call should have been made, not step processing"
    );
    assert!(
        port.calls.last() == Some(&"await_input") || port.calls.last() == Some(&"input"),
        "Last call should be a drain call"
    );
}

/// #1272: InternalContinuation with user input while AwaitingUser
/// DOES resume — the batch carries the user's response.
#[tokio::test]
async fn internal_continuation_while_awaiting_user_with_input_resumes() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        model_steps: VecDeque::from([
            ModelStep::Tools {
                text: "question".to_string(),
                calls: vec![call("AskUserQuestion", json!({}))],
            },
            ModelStep::Complete {
                text: "answered".to_string(),
            },
        ]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    let calls_before = port.calls.len();

    // User input arrives + stop hook fires simultaneously.
    // InternalContinuation carries the user input in batch.
    // #1272: after first run_loop, next_drain_epoch is 1 (EmptyAndSealed
    // during AwaitingUser does NOT advance epoch).
    // InternalContinuation at epoch 1 advances to epoch 2.
    port.drain_outcomes = VecDeque::from([
        DrainOutcome::InternalContinuation {
            kind: InternalContinuationKind::StopHookFeedback {
                feedback: "reminder".to_string(),
            },
            batch: vec![LoopInput {
                text: "yes".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            epoch: DrainEpoch(1),
        },
        DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(2),
        },
    ]);

    run_loop(&mut run, &cancel, &mut port).await.unwrap();
    assert_eq!(run.status(), RunStatus::Completed);
    // New calls were made (step frozen, model invoked, etc.)
    assert!(
        port.calls.len() > calls_before,
        "Should have made new calls after resuming"
    );
    assert!(port.calls.contains(&"freeze_step"));
    assert!(port.calls.contains(&"model"));
}

// ── #1272 terminal text persistence ──────────────────────────────────

/// The last assistant text before EmptyAndSealed MUST be carried in the
/// Completed event.  Previously `terminal_text` was reset to None at
/// the top of each loop iteration, so Complete→EmptyAndSealed lost it.
#[tokio::test]
async fn engine_completed_event_carries_last_assistant_text() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "final answer".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    // The Completed event must carry the assistant text from the model step.
    let completed = port
        .events
        .iter()
        .find_map(|event| match event {
            RunDomainEvent::Completed { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("Completed event must be emitted");
    assert_eq!(
        completed, "final answer",
        "Completed.result must contain the last assistant text"
    );
}

/// Multiple Complete→Continue→Complete steps: only the LAST assistant
/// text survives to the Completed event (not the first).
#[tokio::test]
async fn engine_terminal_text_is_the_last_assistant_text_not_the_first() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "second".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(1),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(2),
            },
        ]),
        model_steps: VecDeque::from([
            ModelStep::Continue {
                text: "not done yet".to_string(),
            },
            ModelStep::Complete {
                text: "now done".to_string(),
            },
        ]),
        ..Default::default()
    };

    run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    let completed = port
        .events
        .iter()
        .find_map(|event| match event {
            RunDomainEvent::Completed { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("Completed event must be emitted");
    assert_eq!(
        completed, "now done",
        "Completed.result must be the LAST assistant text, not the first"
    );
}

// ── #1272 epoch validation tests ─────────────────────────────────────

/// L1: The engine rejects a drain outcome with a wrong epoch.
/// The adapter must return the epoch the engine expects; mismatch
/// returns a Chinese-localized `LoopEngineError::Adapter`.
#[tokio::test]
async fn engine_rejects_wrong_epoch() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    // Default drain_outcomes: Ready(epoch 0) then EmptyAndSealed(epoch 1).
    // This matches the engine's expected sequence: 0→1.
    // We override the first outcome to have epoch 5 — a clear mismatch.
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "test".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(5), // Engine expects 0
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(6),
            },
        ]),
        ..Default::default()
    };

    let err = run_loop(&mut run, &cancel, &mut port).await.unwrap_err();
    assert!(
        matches!(&err, LoopEngineError::Adapter(msg) if msg.contains("drain epoch 不匹配")),
        "Expected Chinese epoch mismatch error, got: {err:?}"
    );
}

// ── #1272 await_user_input epoch preservation tests ──────────────────

/// When AwaitingUser + NoInput, the engine must NOT advance the Run's
/// drain epoch. The buffer stays receptive and the next call uses the
/// same expected epoch.
#[tokio::test]
async fn await_user_input_empty_preserves_run_epoch() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    // First call: Ready(epoch 0) → model → Tools → AwaitUser
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
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

    // #1272: After EmptyAndSealed during AwaitingUser, the Run's drain
    // epoch must NOT have advanced past the Ready consumption.
    // Ready(epoch 0) advanced to 1; EmptyAndSealed during AwaitingUser
    // did NOT advance. So next_drain_epoch is 1 (NOT 2).
    assert_eq!(
        run.next_drain_epoch(),
        1,
        "epoch must NOT advance for EmptyAndSealed during AwaitingUser"
    );
}

/// Same Run: AwaitUser → empty drain (NoInput) → AwaitUser → then user
/// input arrives at the same epoch → re-enter with correct epoch, consume
/// input, complete the Run. Epoch must be continuous with no jump.
#[tokio::test]
async fn await_user_input_empty_then_input_same_epoch_reenter() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            // This EmptyAndSealed will be consumed during AwaitingUser
            // (the legacy path for ScriptedPort). Epoch stays at 1.
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    // First run_loop: consumes Ready(0), executes step → AwaitUser,
    // then consumes EmptyAndSealed(1) during AwaitingUser → returns AwaitUser.
    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.next_drain_epoch(), 1);

    // Simulate: user input arrives. Next drain should work at epoch 1.
    port.drain_outcomes = VecDeque::from([
        DrainOutcome::ready(
            vec![LoopInput {
                text: "user response".to_string(),
                input_id: None,
                images: Vec::new(),
            }],
            DrainEpoch(1),
        ),
        DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(2),
        },
    ]);
    port.model_steps = VecDeque::from([ModelStep::Complete {
        text: "final answer".to_string(),
    }]);

    // Re-enter: same epoch (1), user input consumed, run completes.
    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();
    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Completed);
    // Epoch advanced: Ready(1) → 2, EmptyAndSealed(2) → 3
    assert_eq!(run.next_drain_epoch(), 3);
}

/// When the engine receives a wrong epoch from drain_input (not
/// AwaitingUser), the Run's drain epoch must NOT be advanced because
/// the error path returns before `advance_drain_epoch`.
#[tokio::test]
async fn drain_input_epoch_mismatch_does_not_advance_run_epoch() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        drain_outcomes: VecDeque::from([
            // This outcome has epoch 5 but the port's drain_epoch starts at 0
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "wrong-epoch-input".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(5),
            ),
        ]),
        ..Default::default()
    };

    let epoch_before = run.next_drain_epoch();
    let result = run_loop(&mut run, &cancel, &mut port).await;
    assert!(result.is_err(), "should return epoch mismatch error");
    // The Run's drain epoch must NOT have advanced
    assert_eq!(
        run.next_drain_epoch(),
        epoch_before,
        "epoch must NOT advance on drain_input error"
    );
}

// ── #1272 close-out: empty Ready + default await_user_input tests ─────

/// `DrainOutcome::ready(vec![])` must NOT panic — the assert has been
/// removed and empty-batch detection lives in `run_loop`.
#[test]
fn drain_outcome_ready_empty_does_not_panic() {
    // If this panics, the test itself fails.
    let outcome = DrainOutcome::ready(vec![], DrainEpoch(0));
    match &outcome {
        DrainOutcome::Ready { batch, .. } => assert!(batch.is_empty()),
        _ => panic!("expected Ready variant, got {outcome:?}"),
    }
}

/// When `run_loop` receives an empty `Ready` batch from the adapter, it
/// must return `Err(Adapter)` WITHOUT advancing epoch, transitioning state,
/// or calling `freeze_step` / `invoke_model`.
#[tokio::test]
async fn run_loop_empty_ready_returns_err_without_executing_step() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        // First (and only) drain returns an empty Ready batch.
        drain_outcomes: VecDeque::from([DrainOutcome::Ready {
            batch: vec![],
            epoch: DrainEpoch(0),
        }]),
        // Provide a model step that should NEVER be invoked.
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "should-not-run".to_string(),
        }]),
        ..Default::default()
    };

    let epoch_before = run.next_drain_epoch();
    let result = run_loop(&mut run, &cancel, &mut port).await;

    let err = result.expect_err("empty Ready must produce an error");
    assert!(
        matches!(&err, LoopEngineError::Adapter(msg) if msg.contains("空的 Ready batch")),
        "Expected Chinese empty-Ready Adapter error, got: {err:?}"
    );

    // Epoch must NOT have advanced.
    assert_eq!(
        run.next_drain_epoch(),
        epoch_before,
        "epoch must NOT advance for empty Ready"
    );
    // Run must NOT be terminal (no Completed/Failed transition).
    assert!(
        !run.status().is_terminal(),
        "Run must not be terminal after empty Ready error"
    );
    // freeze_step / model must NOT have been called.
    assert!(
        !port.calls.contains(&"freeze_step"),
        "freeze_step must not be called for empty Ready"
    );
    assert!(
        !port.calls.contains(&"model"),
        "invoke_model must not be called for empty Ready"
    );
}

// ── DrainOnlyPort: implements only drain_input (no await_user_input) ──

/// A minimal port that implements `drain_input` but does NOT override
/// `await_user_input`, relying on the trait default. Used to verify that
/// entering `AwaitingUser` with the default impl returns an Adapter error
/// instead of delegating to `drain_input` (which could seal the buffer).
struct DrainOnlyPort {
    drain_outcomes: VecDeque<DrainOutcome>,
    drain_epoch: DrainEpoch,
    model_steps: VecDeque<ModelStep>,
    tool_steps: VecDeque<ToolStep>,
    events: Vec<RunDomainEvent>,
    calls: Vec<&'static str>,
}

#[async_trait::async_trait]
impl RunLoopPort for DrainOnlyPort {
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.calls.push("drain_input");
        if expected_epoch != self.drain_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch, self.drain_epoch,
            )));
        }
        let outcome =
            self.drain_outcomes
                .pop_front()
                .unwrap_or_else(|| DrainOutcome::EmptyAndSealed {
                    epoch: self.drain_epoch,
                });
        self.drain_epoch = self.drain_epoch.next();
        Ok(outcome)
    }
    // await_user_input: NOT overridden — uses the default impl.
    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        Ok(false)
    }
    async fn compact(&mut self, _cancel: &CancellationToken) -> Result<(), LoopEngineError> {
        Ok(())
    }
    async fn invoke_model(
        &mut self,
        _cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError> {
        self.calls.push("model");
        self.model_steps
            .pop_front()
            .map(|step| (step, StepTokenUsage::default()))
            .ok_or_else(|| LoopEngineError::Adapter("missing model step".to_string()))
    }
    async fn execute_tools(
        &mut self,
        _run_id: &sdk::RunId,
        _step_id: &sdk::RunStepId,
        _calls: &[(ToolCall, ToolGuardDecision)],
        _cancel: &CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        self.calls.push("tools");
        self.tool_steps
            .pop_front()
            .ok_or_else(|| LoopEngineError::Adapter("missing tool step".to_string()))
    }
    async fn on_stuck(&mut self, _decision: &StuckDecision) -> Result<(), LoopEngineError> {
        Ok(())
    }
    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        self.events.extend(events);
        Ok(())
    }
}

/// A port that only implements `drain_input` (no `await_user_input` override)
/// must receive a Chinese Adapter error when the Run enters `AwaitingUser`,
/// NOT a silent delegation to `drain_input` (which would seal the buffer).
#[tokio::test]
async fn default_await_user_input_returns_error_not_delegating_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = DrainOnlyPort {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                DrainEpoch(0),
            ),
            // This would be consumed by drain_input if the default impl
            // delegated — but it should NOT be reached.
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(1),
            },
        ]),
        drain_epoch: DrainEpoch(0),
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        events: Vec::new(),
        calls: Vec::new(),
    };

    let result = run_loop(&mut run, &cancel, &mut port).await;
    let err = result.expect_err("default await_user_input must return Err");

    assert!(
        matches!(&err, LoopEngineError::Adapter(msg)
            if msg.contains("未覆写 await_user_input")),
        "Expected Chinese 'not overridden' Adapter error, got: {err:?}"
    );

    // drain_input was called exactly once (for the first Ready).
    // It must NOT have been called a second time when AwaitingUser
    // triggered await_user_input.
    let drain_count = port.calls.iter().filter(|&&c| c == "drain_input").count();
    assert_eq!(
        drain_count, 1,
        "drain_input must be called exactly once (first Ready), \
         NOT delegated to by await_user_input"
    );
    // Run must be in AwaitingUser (the step produced AwaitUser before
    // the error interrupted the loop).
    assert_eq!(run.status(), RunStatus::AwaitingUser);
}

// ── #1247 typed Run control scenario tests ─────────────────────────────

#[tokio::test]
async fn terminate_run_during_compaction_finishes_as_terminated() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        terminate_when_compact_starts: true,
        ..Default::default()
    };

    let directive = run_loop(&mut run, &root, &mut port).await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(port.calls.contains(&"compact"));
    assert!(!port.calls.contains(&"model"));
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Terminated { .. })));
}

#[tokio::test]
async fn cancel_step_during_model_finalizes_then_returns_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        cancel_when_model_starts: true,
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "should-not-complete".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(&mut run, &root, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(port.calls.contains(&"model"));
    assert_eq!(port.cancelled_steps, port.frozen_steps);
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::StepCancelled { .. })));
}

#[tokio::test]
async fn cancel_step_during_tools_finalizes_then_returns_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        cancel_when_tools_starts: true,
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "calling".to_string(),
            calls: vec![call("Read", json!({"file_path": "a.rs"}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::Continue]),
        ..Default::default()
    };

    run_loop(&mut run, &root, &mut port).await.unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(port.calls.contains(&"tools"));
    assert_eq!(port.cancelled_steps, port.frozen_steps);
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::StepCancelled { .. })));
}

#[tokio::test]
async fn terminate_while_awaiting_user_finishes_as_terminated() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    // First run_loop: enters AwaitingUser.
    let directive = run_loop(&mut run, &root, &mut port).await.unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);

    // AwaitUser 前的 step outcome 必须已被 finalize（持久化）。
    // 否则 Terminate 时 active_step 为 None，step 的模型回复会永久丢失。
    assert_eq!(
        port.finalized_steps.len(),
        1,
        "AwaitUser 前的 step 必须已 finalize，否则 Terminate 时 outcome 丢失"
    );

    // Inject TerminateRun control; root cancel fires so drain is interrupted.
    port.controls
        .lock()
        .unwrap()
        .push_back(RunControl::Terminate {
            reason: sdk::RunTerminationReason::SessionShutdown,
            deadline: sdk::ControlDeadline::from_unix_millis(1_725_000_000_789),
        });
    root.cancel();

    let directive = run_loop(&mut run, &root, &mut port).await.unwrap();
    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Terminated { .. })));
}
