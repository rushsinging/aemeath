use super::*;
use crate::application::agent::ToolCall;
use sdk::ChatInputEvent;
use serde_json::json;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::domain::agent_run::{Run, RunControl, RunDomainEvent, RunSpec, RunStatus};

#[derive(Default)]
struct ScriptedPort {
    model_steps: VecDeque<ModelStep>,
    model_errors: VecDeque<LoopEngineError>,
    tool_steps: VecDeque<ToolStep>,
    controls: VecDeque<RunControl>,
    control_after_model_calls: Option<usize>,
    calls: Vec<&'static str>,
    events: Vec<RunDomainEvent>,
    guarded_calls: Vec<Vec<ToolGuardDecision>>,
    input_batches: VecDeque<Vec<LoopInput>>,
    cancelled_during_model: bool,
    block_model_forever: bool,
    block_compact_until_cancelled: bool,
    block_tools_until_cancelled: bool,
    control_on_cancel: Option<RunControl>,
    blocked_stage: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    cancelled_steps: Vec<sdk::RunStepId>,
    finalized_steps: Vec<sdk::RunStepId>,
    frozen_steps: Vec<sdk::RunStepId>,
    needs_compaction: bool,
    fail_emit_once: bool,
    fail_cancelled_finalization_once: bool,
}

#[async_trait::async_trait]
impl RunLoopPort for ScriptedPort {
    async fn drain_input(&mut self) -> Result<Vec<LoopInput>, LoopEngineError> {
        self.calls.push("input");
        Ok(self.input_batches.pop_front().unwrap_or_default())
    }

    fn freeze_step(&mut self, step_id: &sdk::RunStepId, _inputs: &[LoopInput]) {
        self.calls.push("freeze_step");
        self.frozen_steps.push(step_id.clone());
    }

    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        self.calls.push("needs_compaction");
        Ok(self.needs_compaction)
    }

    async fn compact(&mut self, cancel: &CancellationToken) -> Result<(), LoopEngineError> {
        self.calls.push("compact");
        if self.block_compact_until_cancelled {
            if let Some(blocked) = &self.blocked_stage {
                blocked.store(true, std::sync::atomic::Ordering::SeqCst);
            }
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
            if let Some(blocked) = &self.blocked_stage {
                blocked.store(true, std::sync::atomic::Ordering::SeqCst);
            }
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
        if self.fail_cancelled_finalization_once {
            self.fail_cancelled_finalization_once = false;
            return Err(LoopEngineError::Adapter("finalization failed".to_string()));
        }
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
        if self.block_tools_until_cancelled {
            if let Some(blocked) = &self.blocked_stage {
                blocked.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            cancel.cancelled().await;
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

    fn take_control(&mut self, _run_id: &sdk::RunId) -> Option<RunControl> {
        if let Some(control) = self.control_on_cancel.clone() {
            let interrupted = self
                .blocked_stage
                .as_ref()
                .is_some_and(|blocked| blocked.load(std::sync::atomic::Ordering::SeqCst));
            if interrupted {
                self.control_on_cancel = None;
                return Some(control);
            }
        }
        if let Some(required) = self.control_after_model_calls {
            let model_calls = self.calls.iter().filter(|call| **call == "model").count();
            if model_calls < required {
                return None;
            }
        }
        self.controls.pop_front()
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

#[tokio::test]
async fn cancel_step_finalizes_then_drains_without_cancelling_the_run() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        controls: VecDeque::from([RunControl::CancelStep]),
        control_after_model_calls: Some(1),
        model_steps: VecDeque::from([
            ModelStep::Complete {
                text: "cancelled output".to_string(),
            },
            ModelStep::Complete {
                text: "done".to_string(),
            },
        ]),
        input_batches: VecDeque::from([
            vec![LoopInput {
                text: "first".to_string(),
            }],
            vec![LoopInput {
                text: "after cancel".to_string(),
            }],
        ]),
        ..Default::default()
    };

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(
        run.status(),
        RunStatus::Completed,
        "calls={:?} events={:?}",
        port.calls,
        port.events
    );
    assert_eq!(run.steps().len(), 2);
    assert_eq!(
        run.steps()[0].status(),
        crate::domain::agent_run::RunStepStatus::Cancelled
    );
    assert_eq!(
        run.steps()[1].status(),
        crate::domain::agent_run::RunStepStatus::Done
    );
    assert_eq!(port.cancelled_steps, vec![port.frozen_steps[0].clone()]);
    let control_events = port
        .events
        .iter()
        .filter_map(|event| match event {
            RunDomainEvent::StepCancellationRequested { .. } => Some("requested"),
            RunDomainEvent::StepFinalizationStarted { .. } => Some("finalizing"),
            RunDomainEvent::StepCancelled { .. } => Some("cancelled"),
            RunDomainEvent::DrainingInput { .. } => Some("draining"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        control_events,
        vec!["requested", "finalizing", "cancelled", "draining"]
    );
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
}

#[tokio::test]
async fn terminate_run_finishes_as_terminated_with_the_requested_reason() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let deadline = sdk::ControlDeadline::from_unix_millis(5_000);
    let mut port = ScriptedPort {
        controls: VecDeque::from([RunControl::Terminate {
            reason: sdk::RunTerminationReason::UserExit,
            deadline,
        }]),
        control_after_model_calls: Some(1),
        model_steps: VecDeque::from([ModelStep::Continue {
            text: "continue".to_string(),
        }]),
        input_batches: VecDeque::from([vec![LoopInput {
            text: "first".to_string(),
        }]]),
        ..Default::default()
    };

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(port.events.iter().any(|event| matches!(
        event,
        RunDomainEvent::Terminated {
            reason: sdk::RunTerminationReason::UserExit,
            ..
        }
    )));
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
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
            "needs_compaction",
            "emit",
            "model",
            "finalize_step",
            "emit",
        ]
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
            "needs_compaction",
            "emit",
            "model",
            "compact",
            "model",
            "finalize_step",
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
async fn terminate_control_wins_when_compaction_is_interrupted() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let deadline = sdk::ControlDeadline::from_unix_millis(5_000);
    let blocked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut port = ScriptedPort {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        input_batches: VecDeque::from([vec![LoopInput {
            text: "start".to_string(),
        }]]),
        blocked_stage: Some(blocked.clone()),
        control_on_cancel: Some(RunControl::Terminate {
            reason: sdk::RunTerminationReason::UserExit,
            deadline,
        }),
        ..Default::default()
    };
    let cancel_for_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        while !blocked.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        cancel_for_task.cancel();
    });

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();
    canceller.await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
}

#[tokio::test]
async fn terminate_control_wins_when_model_is_interrupted() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let deadline = sdk::ControlDeadline::from_unix_millis(5_000);
    let blocked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut port = ScriptedPort {
        cancelled_during_model: true,
        input_batches: VecDeque::from([vec![LoopInput {
            text: "start".to_string(),
        }]]),
        blocked_stage: Some(blocked.clone()),
        control_on_cancel: Some(RunControl::Terminate {
            reason: sdk::RunTerminationReason::UserExit,
            deadline,
        }),
        ..Default::default()
    };
    let cancel_for_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        while !blocked.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        cancel_for_task.cancel();
    });

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();
    canceller.await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(!port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Cancelled { .. })));
}

#[tokio::test]
async fn terminate_control_wins_when_tool_is_interrupted() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let deadline = sdk::ControlDeadline::from_unix_millis(5_000);
    let blocked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "tool".to_string(),
            calls: vec![call("CustomTool", json!({}))],
        }]),
        input_batches: VecDeque::from([vec![LoopInput {
            text: "start".to_string(),
        }]]),
        tool_steps: VecDeque::new(),
        block_tools_until_cancelled: true,
        blocked_stage: Some(blocked.clone()),
        control_on_cancel: Some(RunControl::Terminate {
            reason: sdk::RunTerminationReason::UserExit,
            deadline,
        }),
        ..Default::default()
    };
    let cancel_for_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        while !blocked.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        cancel_for_task.cancel();
    });

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();
    canceller.await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(!port
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
async fn cancel_step_after_stop_hook_block_finalizes_without_persisting_completed_step() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        controls: VecDeque::from([RunControl::CancelStep]),
        control_after_model_calls: Some(1),
        model_steps: VecDeque::from([ModelStep::StopHookBlocked {
            text: "blocked".to_string(),
        }]),
        input_batches: VecDeque::from([vec![LoopInput {
            text: "first".to_string(),
        }]]),
        ..Default::default()
    };

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(port.cancelled_steps, port.frozen_steps);
    assert!(port.finalized_steps.is_empty());
}

#[tokio::test]
async fn terminate_finalizer_error_still_closes_run_as_terminated() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        controls: VecDeque::from([RunControl::Terminate {
            reason: sdk::RunTerminationReason::SessionShutdown,
            deadline: sdk::ControlDeadline::from_unix_millis(5_000),
        }]),
        control_after_model_calls: Some(1),
        model_steps: VecDeque::from([ModelStep::Continue {
            text: "partial".to_string(),
        }]),
        input_batches: VecDeque::from([vec![LoopInput {
            text: "start".to_string(),
        }]]),
        fail_cancelled_finalization_once: true,
        ..Default::default()
    };

    let error = run_loop(&mut run, &cancel, &mut port).await.unwrap_err();

    assert!(matches!(error, LoopEngineError::Adapter(_)));
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(port
        .events
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Terminated { .. })));
}

#[tokio::test]
async fn terminate_while_awaiting_user_finishes_as_terminated() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedPort {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        input_batches: VecDeque::from([vec![LoopInput {
            text: "start".to_string(),
        }]]),
        ..Default::default()
    };
    assert_eq!(
        run_loop(&mut run, &cancel, &mut port).await.unwrap(),
        LoopDirective::AwaitUser
    );
    port.controls.push_back(RunControl::Terminate {
        reason: sdk::RunTerminationReason::SessionShutdown,
        deadline: sdk::ControlDeadline::from_unix_millis(5_000),
    });

    let directive = run_loop(&mut run, &cancel, &mut port).await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
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
            RunDomainEvent::Started { .. }
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
