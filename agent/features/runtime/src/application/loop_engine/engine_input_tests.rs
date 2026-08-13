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
    let mut port = ScriptedScenario {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "test".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
                }],
                DrainEpoch(5), // Engine expects 0
            ),
            DrainOutcome::EmptyAndSealed {
                epoch: DrainEpoch(6),
            },
        ]),
        ..Default::default()
    };

    let err = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap_err();
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
    let mut port = ScriptedScenario {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
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

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();
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
    let mut port = ScriptedScenario {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
                }],
                DrainEpoch(0),
            ),
            // This EmptyAndSealed will be consumed during AwaitingUser
            // (the legacy path for ScriptedScenario). Epoch stays at 1.
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
    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.next_drain_epoch(), 1);

    // Simulate: user input arrives. Next drain should work at epoch 1.
    port.drain_outcomes = VecDeque::from([
        DrainOutcome::ready(
            vec![LoopInput {
                text: "user response".to_string(),
                input_id: None,
                images: Vec::new(),
                accepted: None,
            }],
            DrainEpoch(1),
        ),
        DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(2),
        },
    ]);
    port.sync_inputs();
    port.model_steps = VecDeque::from([ModelStep::Complete {
        text: "final answer".to_string(),
    }]);
    port.sync_inputs();

    // Re-enter: same epoch (1), user input consumed, run completes.
    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();
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
    let mut port = ScriptedScenario {
        drain_outcomes: VecDeque::from([
            // This outcome has epoch 5 but the port's drain_epoch starts at 0
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "wrong-epoch-input".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
                }],
                DrainEpoch(5),
            ),
        ]),
        ..Default::default()
    };

    let epoch_before = run.next_drain_epoch();
    let result = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await;
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
    let mut port = ScriptedScenario {
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
    let result = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await;

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
        !port.calls().contains(&"freeze_step"),
        "freeze_step must not be called for empty Ready"
    );
    assert!(
        !port.calls().contains(&"model"),
        "invoke_model must not be called for empty Ready"
    );
}

// ── DrainInputFake: only the input seam omits await_user_input ──

/// A minimal input fake that implements `drain_input` but does NOT override
/// `await_user_input`, relying on the trait default. Other capabilities remain
/// independent narrow fakes from `ScriptedPorts`.
struct DrainInputFake {
    state: Arc<std::sync::Mutex<ScriptedState>>,
}

#[async_trait::async_trait]
impl InputPort for DrainInputFake {
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        let mut state = self.state.lock().unwrap();
        state.observations.calls.push("drain_input");
        if expected_epoch != state.drain_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch, state.drain_epoch,
            )));
        }
        let epoch = state.drain_epoch;
        let outcome = state
            .drain_outcomes
            .pop_front()
            .unwrap_or(DrainOutcome::EmptyAndSealed { epoch });
        state.drain_epoch = state.drain_epoch.next();
        Ok(outcome)
    }
}

/// A port that only implements `drain_input` (no `await_user_input` override)
/// must receive a Chinese Adapter error when the Run enters `AwaitingUser`,
/// NOT a silent delegation to `drain_input` (which would seal the buffer).
#[tokio::test]
async fn default_await_user_input_returns_error_not_delegating_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut scenario = ScriptedScenario {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "first".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
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
        ..Default::default()
    };
    scenario.ports();
    let state = Arc::clone(&scenario.state);
    let mut drain_input = DrainInputFake {
        state: Arc::clone(&state),
    };
    let ports = scenario.ports.as_mut().expect("scripted ports must exist");
    let mut loop_context = RunLoop::new(
        &mut drain_input,
        &mut ports.events,
        &ports.control,
        &ports.lifecycle,
        &mut ports.interaction,
        &mut ports.persistence,
        &mut ports.compaction,
        &mut ports.model,
        &mut ports.stop_hook,
        &mut ports.tools,
        &mut ports.stuck,
        &ports.plan_approval,
    );

    loop_context.bind_test_activity_context();

    let result = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut loop_context,
    )
    .await;
    let err = result.expect_err("default await_user_input must return Err");

    assert!(
        matches!(&err, LoopEngineError::Adapter(msg)
            if msg.contains("未覆写 await_user_input")),
        "Expected Chinese 'not overridden' Adapter error, got: {err:?}"
    );

    let calls = &state.lock().unwrap().observations.calls;
    let drain_count = calls.iter().filter(|&&call| call == "drain_input").count();
    assert_eq!(
        drain_count, 1,
        "drain_input must be called exactly once (first Ready), \
         NOT delegated to by await_user_input"
    );
    assert_eq!(run.status(), RunStatus::AwaitingUser);
}

// ── #1247 typed Run control scenario tests ─────────────────────────────

#[tokio::test]
async fn terminate_run_during_compaction_finishes_as_terminated() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedScenario {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        terminate_when_compact_starts: true,
        ..Default::default()
    };

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(port.calls().contains(&"compact"));
    assert!(!port.calls().contains(&"model"));
    assert_eq!(
        port.events()
            .iter()
            .filter(|event| matches!(event, RuntimeLifecycleEvent::Terminated { .. }))
            .count(),
        1,
        "Run termination must emit exactly one terminal domain event"
    );
}

#[tokio::test]
async fn cancel_step_during_model_finalizes_then_returns_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedScenario {
        cancel_when_model_starts: true,
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "should-not-complete".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(port.calls().contains(&"model"));
    assert_eq!(port.cancelled_steps(), port.frozen_steps());
    assert!(!port
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::Terminated { .. })));
    assert!(port
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::StepCancelled { .. })));
}

#[tokio::test]
async fn cancel_step_during_model_waits_for_stream_cancellation_cleanup() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedScenario {
        cancel_when_model_starts: true,
        require_model_cancellation_cleanup: true,
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert!(
        port.state
            .lock()
            .unwrap()
            .model_cancellation_cleanup_completed,
        "model stream cancellation must pass through its cleanup path before Step finalization"
    );
    assert_eq!(port.cancelled_steps(), port.frozen_steps());
}

#[tokio::test]
async fn cancel_step_during_tools_finalizes_then_returns_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedScenario {
        cancel_when_tools_starts: true,
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "calling".to_string(),
            calls: vec![call("Read", json!({"file_path": "a.rs"}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::Continue]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(port.calls().contains(&"tools"));
    assert_eq!(port.cancelled_steps(), port.frozen_steps());
    assert!(!port
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::Terminated { .. })));
    assert!(port
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::StepCancelled { .. })));
}

#[tokio::test]
async fn terminate_while_awaiting_user_finishes_as_terminated() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedScenario {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };

    // First run_loop: enters AwaitingUser.
    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);

    // AwaitUser 前的 step outcome 必须已被 finalize（持久化）。
    // 否则 Terminate 时 active_step 为 None，step 的模型回复会永久丢失。
    assert_eq!(
        port.finalized_steps().len(),
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

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &root,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();
    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(port
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::Terminated { .. })));
}

// ═══════════════════════════════════════════════════════════════════
// #1248 Task 5: Four-body interaction routing engine tests (RED)
// ═══════════════════════════════════════════════════════════════════

mod interaction_routing {
    use super::*;
    use crate::application::loop_engine::{
        ApprovalRequiredCall, SuspendedQuestion, SuspendedToolCall,
    };

    /// Helper: Create a port+run with Tools model step and given tool_step.
    fn setup_tool_run(
        model_step: ModelStep,
        tool_step: ToolStep,
    ) -> (Run, CancellationToken, ScriptedScenario) {
        let run = Run::new(RunSpec::main(), None);
        let root = CancellationToken::new();
        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "user input".to_string(),
                input_id: None,
                images: Vec::new(),
                accepted: None,
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let port = ScriptedScenario {
            model_steps: VecDeque::from([model_step]),
            tool_steps: VecDeque::from([tool_step]),
            drain_outcomes: drain_q,
            ..Default::default()
        };
        (run, root, port)
    }

    // ── UserQuestions: InteractionSuspended → engine creates intent ──

    /// InteractionSuspended registers via coordinator, publishes to UI,
    /// stores receiver, and returns AwaitUser.
    #[tokio::test]
    async fn user_questions_suspension_creates_awaiting_user() {
        let call = call("AskUserQuestion", json!({"question": "continue?"}));
        let suspended = SuspendedToolCall {
            call: call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "continue?".to_string(),
                options: vec![
                    sdk::OptionItem::new("yes", "approve"),
                    sdk::OptionItem::new("no", "decline"),
                ],
                allow_multi: false,
            }],
        };

        let (mut run, root, mut port) = setup_tool_run(
            ModelStep::Tools {
                text: String::new(),
                calls: vec![call],
            },
            ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended],
            },
        );

        let directive = run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &root,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);
        assert!(run.pending_interaction().is_some());
        assert!(
            port.calls().contains(&"publish_interaction"),
            "should have published: {:?}",
            port.calls()
        );
    }

    // ── Continuation identity ──

    /// InteractionSuspended preserves CompleteToolCall continuation with
    /// the call ID from the suspended tool call.
    #[tokio::test]
    async fn interaction_suspended_preserves_continuation_identity() {
        let call_id = sdk::ids::ToolCallId::from_legacy_or_new("my-call-id");
        let call = ToolCall {
            id: call_id.clone(),
            provider_id: "provider-1".to_string(),
            name: "AskUserQuestion".to_string(),
            index: 0,
            input: json!({"question": "q"}),
        };

        let suspended = SuspendedToolCall {
            call: call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q".to_string(),
                options: vec![sdk::OptionItem::new("a", "approve now")],
                allow_multi: false,
            }],
        };

        let (mut run, root, mut port) = setup_tool_run(
            ModelStep::Tools {
                text: String::new(),
                calls: vec![call],
            },
            ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended],
            },
        );

        let directive = run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &root,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);

        let pending = run
            .pending_interaction()
            .expect("should have pending interaction");
        assert_eq!(
            pending.continuation,
            InteractionContinuation::CompleteToolCall(call_id)
        );

        // Verify published interaction has UserQuestions body and the option
        // description survives the engine boundary (issue: description must
        // flow tools → runtime → sdk without loss).
        let published = port.published_interactions.lock().unwrap();
        assert_eq!(published.len(), 1);
        match &published[0].body {
            sdk::InteractionRequestBody::UserQuestions(questions) => {
                assert_eq!(questions.len(), 1);
                assert_eq!(questions[0].options.len(), 1);
                assert_eq!(questions[0].options[0].title, "a");
                assert_eq!(
                    questions[0].options[0].description.as_deref(),
                    Some("approve now")
                );
            }
            other => panic!("expected UserQuestions body, got {other:?}"),
        }
    }

    // ── L2: ToolApproval: AwaitingToolApproval → coordinator ──

    /// AwaitingToolApproval creates ToolApproval intent via coordinator,
    /// stores the receiver, and returns AwaitUser.
    #[tokio::test]
    async fn tool_approval_creates_awaiting_user() {
        let call = call("Bash", json!({"command": "ls"}));
        let call_id = call.id.clone();
        let approval = ApprovalRequiredCall {
            call: call.clone(),
            authorization: tools::AuthorizationContext::STANDARD,
            reason: "approval required: high risk".to_string(),
            subject: "exec".to_string(),
        };

        let (mut run, root, mut port) = setup_tool_run(
            ModelStep::Tools {
                text: String::new(),
                calls: vec![call],
            },
            ToolStep::AwaitingToolApproval {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                calls_needing_approval: vec![approval],
            },
        );

        let directive = run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &root,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert!(run.pending_interaction().is_some());
        let pending = run.pending_interaction().unwrap();
        assert_eq!(
            pending.continuation,
            InteractionContinuation::ContinueToolApproval(call_id)
        );
    }

    // ── Multi-suspension: serial two AskUserQuestion ──

    /// Two AskUserQuestion calls: only the first is started immediately;
    /// the second is queued via PendingInteractionWork.
    #[tokio::test]
    async fn multi_suspension_queues_second_and_does_not_complete_step() {
        let call1 = call("AskUserQuestion", json!({"question": "q1"}));
        let call2 = call("AskUserQuestion", json!({"question": "q2"}));
        let suspended1 = SuspendedToolCall {
            call: call1.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q1".to_string(),
                options: vec![sdk::OptionItem::new("a", "first")],
                allow_multi: false,
            }],
        };
        let suspended2 = SuspendedToolCall {
            call: call2.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q2".to_string(),
                options: vec![sdk::OptionItem::new("b", "second")],
                allow_multi: false,
            }],
        };

        let (mut run, root, mut port) = setup_tool_run(
            ModelStep::Tools {
                text: String::new(),
                calls: vec![call1, call2],
            },
            ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended1, suspended2],
            },
        );

        let directive = run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &root,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert!(run.pending_interaction().is_some());
        // Only one interaction was started; the second is queued on the port
        let pending_work = port.pending_work.lock().unwrap();
        assert!(
            pending_work.is_some(),
            "second suspension should be queued via set_pending_interaction_work"
        );
        let work = pending_work.as_ref().unwrap();
        assert_eq!(work.queue.len(), 1, "one item should be in the queue");
    }

    // ── RequireApproval: full engine roundtrip approve ──

    /// Full engine roundtrip for tool approval: setup fake_tool_port,
    /// first run_loop → AwaitUser, reply approve via bridge,
    /// second run_loop → tool executes → Success.
    #[tokio::test]
    async fn require_approval_approve_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();
        let fake = Arc::new(FakeToolExecutionPort::new());
        fake.set_result_text("approved result");

        let call = call("Bash", json!({"command": "ls"}));
        let call_id = call.id.clone();

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "run ls".to_string(),
                input_id: None,
                images: Vec::new(),
                accepted: None,
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedScenario {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::AwaitingToolApproval {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                calls_needing_approval: vec![ApprovalRequiredCall {
                    call: call.clone(),
                    authorization: tools::AuthorizationContext::STANDARD,
                    reason: "dangerous".to_string(),
                    subject: "exec".to_string(),
                }],
            }]),
            drain_outcomes: drain_q,
            fake_tool_port: Some(fake.clone()),
            ..Default::default()
        };

        // First run_loop: engine creates ToolApproval intent → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // Get the request_id from execution-owned mailbox metadata
        let request_id = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();

        // Reply approve via the interaction bridge
        let reply = sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Approve);
        let outcome = port.interaction_bridge.reply(&request_id, reply);
        assert_eq!(outcome, sdk::InteractionCommandOutcome::Accepted);

        // Set up drain outcomes for second run_loop: complete after resolution
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        port.sync_inputs();

        // Second run_loop: polls resolved interaction, finishes work, completes
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Assertions: tool executed once with correct invocation
        assert_eq!(fake.execute_count(), 1);
        let invocations = fake.recorded_invocations.lock().unwrap();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].tool_name.as_str(), "Bash");
        assert_eq!(invocations[0].input, json!({"command": "ls"}));
        assert_eq!(
            invocations[0].authorization,
            tools::AuthorizationContext::STANDARD
        );
        // set_result_text was used — verify the returned text
        assert_eq!(fake.returned_text(), Some("approved result".to_string()));

        // Verify the tool call has Success status in the Run
        let step = &run.steps()[0];
        assert_eq!(step.tool_calls().len(), 1);
        assert_eq!(step.tool_calls()[0].status(), ToolCallStatus::Success);
        assert_eq!(step.tool_calls()[0].id(), &call_id);
    }

    // ── RequireApproval: full engine roundtrip deny ──

    /// Full engine roundtrip for tool approval deny:
    /// first run_loop → AwaitUser, reply deny via bridge,
    /// second run_loop → tool NOT executed, Cancelled.
    #[tokio::test]
    async fn require_approval_deny_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();
        let fake = Arc::new(FakeToolExecutionPort::new());

        let call = call("Bash", json!({"command": "rm -rf /"}));
        let call_id = call.id.clone();

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "dangerous cmd".to_string(),
                input_id: None,
                images: Vec::new(),
                accepted: None,
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedScenario {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::AwaitingToolApproval {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                calls_needing_approval: vec![ApprovalRequiredCall {
                    call: call.clone(),
                    authorization: tools::AuthorizationContext::STANDARD,
                    reason: "dangerous".to_string(),
                    subject: "destroy".to_string(),
                }],
            }]),
            drain_outcomes: drain_q,
            fake_tool_port: Some(fake.clone()),
            ..Default::default()
        };

        // First run_loop → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);

        // Reply deny via the interaction bridge
        let request_id = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply =
            sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Deny { reason: None });
        let outcome = port.interaction_bridge.reply(&request_id, reply);
        assert_eq!(outcome, sdk::InteractionCommandOutcome::Accepted);

        // Second run_loop: resolve → Cancelled
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        port.sync_inputs();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Assertions: tool was NOT executed
        assert_eq!(fake.execute_count(), 0);

        // Tool call status is Cancelled
        let step = &run.steps()[0];
        assert_eq!(step.tool_calls().len(), 1);
        assert_eq!(step.tool_calls()[0].status(), ToolCallStatus::Cancelled);
        assert_eq!(step.tool_calls()[0].id(), &call_id);
    }

    // ── UserQuestions: single question full roundtrip ──

    /// Interaction reply must wake a Run that is concurrently parked on the
    /// Session input mailbox. This reproduces the production deadlock where
    /// the oneshot completed but `await_user_input` never returned.
    #[tokio::test]
    async fn interaction_reply_wakes_run_while_session_input_is_pending() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();
        let call = call("AskUserQuestion", json!({"question": "continue?"}));
        let call_id = call.id.clone();
        let suspended = SuspendedToolCall {
            call: call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "continue?".to_string(),
                options: vec![
                    sdk::OptionItem::new("yes", "approve"),
                    sdk::OptionItem::new("no", "decline"),
                ],
                allow_multi: false,
            }],
        };
        let mut port = ScriptedScenario {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call],
            }]),
            tool_steps: VecDeque::from([ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended],
            }]),
            block_await_user_input_forever: true,
            ..Default::default()
        };
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();

        let bridge = Arc::clone(&port.interaction_bridge);
        let published = Arc::clone(&port.published_interactions);
        let reply_task = tokio::spawn(async move {
            loop {
                let request_id = published
                    .lock()
                    .unwrap()
                    .first()
                    .map(|request| request.id.clone());
                if let Some(request_id) = request_id {
                    return bridge.reply(
                        &request_id,
                        sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer(
                            "yes".to_string(),
                        )]),
                    );
                }
                tokio::task::yield_now().await;
            }
        });

        let directive = tokio::time::timeout(
            Duration::from_millis(200),
            run_loop(
                &mut run,
                &mut execution,
                &cancel,
                &mut scripted_run_loop(&mut port),
            ),
        )
        .await
        .expect("interaction reply must wake the Run without Session input")
        .unwrap();
        assert_eq!(
            reply_task.await.unwrap(),
            sdk::InteractionCommandOutcome::Accepted
        );
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);
        assert_eq!(run.steps()[0].tool_calls()[0].id(), &call_id);
        assert_eq!(
            run.steps()[0].tool_calls()[0].status(),
            ToolCallStatus::Success
        );
    }

    /// Full engine roundtrip for a single UserQuestions interaction:
    /// run_loop → AwaitUser, reply via bridge, re-enter → Success.
    #[tokio::test]
    async fn user_questions_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();

        let call = call("AskUserQuestion", json!({"question": "continue?"}));
        let call_id = call.id.clone();
        let suspended = SuspendedToolCall {
            call: call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "continue?".to_string(),
                options: vec![
                    sdk::OptionItem::new("yes", "approve"),
                    sdk::OptionItem::new("no", "decline"),
                ],
                allow_multi: false,
            }],
        };

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "ask question".to_string(),
                input_id: None,
                images: Vec::new(),
                accepted: None,
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedScenario {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended],
            }]),
            drain_outcomes: drain_q,
            ..Default::default()
        };

        // First run_loop → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // Reply via bridge
        let request_id = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply = sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("yes".to_string())]);
        let outcome = port.interaction_bridge.reply(&request_id, reply);
        assert_eq!(outcome, sdk::InteractionCommandOutcome::Accepted);

        // Second run_loop: resolve → Success
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        port.sync_inputs();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Tool call is Success
        let step = &run.steps()[0];
        assert_eq!(step.tool_calls().len(), 1);
        assert_eq!(step.tool_calls()[0].status(), ToolCallStatus::Success);
        assert_eq!(step.tool_calls()[0].id(), &call_id);
    }

    /// Cancelling the active UserQuestions interaction remains interaction-scoped,
    /// completes the current Step exactly once, and never applies ToolsCompleted
    /// while the Run is still AwaitingUser.
    #[tokio::test]
    async fn user_questions_cancel_completes_step_once_without_illegal_transition() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();
        let call = call("AskUserQuestion", json!({"question": "continue?"}));
        let call_id = call.id.clone();
        let suspended = SuspendedToolCall {
            call: call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "continue?".to_string(),
                options: vec![
                    sdk::OptionItem::new("yes", "approve"),
                    sdk::OptionItem::new("no", "decline"),
                ],
                allow_multi: false,
            }],
        };
        let mut port = ScriptedScenario {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call],
            }]),
            tool_steps: VecDeque::from([ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended],
            }]),
            ..Default::default()
        };
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();

        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        let request_id = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        assert_eq!(
            port.interaction_bridge
                .cancel(&request_id, sdk::InteractionCancelReason::UserCancelled,),
            sdk::InteractionCommandOutcome::Accepted
        );
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        port.sync_inputs();

        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .expect("interaction cancellation must leave AwaitingUser before ToolsCompleted");

        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);
        assert!(run.pending_interaction().is_none());
        assert_eq!(run.steps().len(), 1);
        assert_eq!(run.steps()[0].tool_calls()[0].id(), &call_id);
        assert_eq!(
            run.steps()[0].tool_calls()[0].status(),
            ToolCallStatus::Cancelled
        );
        assert_eq!(port.finalized_steps().len(), 1);
        assert!(!port
            .events()
            .iter()
            .any(|event| matches!(event, RuntimeLifecycleEvent::Resumed { .. })));
        assert_eq!(
            port.events()
                .iter()
                .filter(|event| matches!(event, RuntimeLifecycleEvent::Completed { .. }))
                .count(),
            1
        );
    }

    // ── UserQuestions: two questions serial roundtrip ──

    /// Two AskUserQuestion calls: first resolved → second becomes active,
    /// second resolved → step completes. No direct finish seam.
    #[tokio::test]
    async fn user_questions_two_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();

        let call1 = call("AskUserQuestion", json!({"question": "q1"}));
        let call2 = call("AskUserQuestion", json!({"question": "q2"}));
        let call1_id = call1.id.clone();
        let call2_id = call2.id.clone();

        let suspended1 = SuspendedToolCall {
            call: call1.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q1".to_string(),
                options: vec![sdk::OptionItem::new("a", "first")],
                allow_multi: false,
            }],
        };
        let suspended2 = SuspendedToolCall {
            call: call2.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "q2".to_string(),
                options: vec![sdk::OptionItem::new("b", "second")],
                allow_multi: false,
            }],
        };

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "ask two".to_string(),
                input_id: None,
                images: Vec::new(),
                accepted: None,
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedScenario {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![call1.clone(), call2.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::InteractionSuspended {
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended1, suspended2],
            }]),
            drain_outcomes: drain_q,
            ..Default::default()
        };

        // First run_loop: first question active, second queued → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // Reply to first question via bridge
        let request_id1 = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply1 = sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("a".to_string())]);
        assert_eq!(
            port.interaction_bridge.reply(&request_id1, reply1),
            sdk::InteractionCommandOutcome::Accepted
        );

        // Second run_loop: resolve first, start second → AwaitUser again
        port.drain_outcomes = VecDeque::from([DrainOutcome::NoInput {
            epoch: DrainEpoch(1),
        }]);
        port.sync_inputs();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // First call now Success, step still active (second interaction pending)
        let step = &run.steps()[0];
        let tc1 = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &call1_id)
            .unwrap();
        assert_eq!(tc1.status(), ToolCallStatus::Success);
        assert!(
            run.active_step_id().is_some(),
            "step should still be active while second interaction is pending"
        );

        // Reply to second question via bridge
        let request_id2 = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply2 = sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("b".to_string())]);
        assert_eq!(
            port.interaction_bridge.reply(&request_id2, reply2),
            sdk::InteractionCommandOutcome::Accepted
        );

        // Third run_loop: resolve second, complete step → terminal
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        port.sync_inputs();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Both calls are Success, step is completed
        let step = &run.steps()[0];
        let tc1 = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &call1_id)
            .unwrap();
        let tc2 = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &call2_id)
            .unwrap();
        assert_eq!(tc1.status(), ToolCallStatus::Success);
        assert_eq!(tc2.status(), ToolCallStatus::Success);
        assert!(
            run.active_step_id().is_none(),
            "step should be completed after all interactions resolve"
        );
    }

    // ── Mixed: completed_results + suspended roundtrip ──

    /// Mixed round: one non-interaction call + one suspended question.
    /// First run_loop: non-interaction already Success, suspension creates AwaitUser.
    /// After reply: suspension becomes Success, non-interaction is NOT re-advanced.
    #[tokio::test]
    async fn mixed_completed_and_suspension_full_roundtrip() {
        let mut run = Run::new(RunSpec::main(), None);
        let cancel = CancellationToken::new();

        let bash_call = call("Bash", json!({"command": "ls"}));
        let question_call = call("AskUserQuestion", json!({"question": "go?"}));
        let bash_id = bash_call.id.clone();
        let question_id = question_call.id.clone();

        let suspended_q = SuspendedToolCall {
            call: question_call.clone(),
            questions: vec![SuspendedQuestion {
                prompt: "go?".to_string(),
                options: vec![sdk::OptionItem::new("yes", "approve")],
                allow_multi: false,
            }],
        };

        let mut drain_q = VecDeque::new();
        drain_q.push_back(DrainOutcome::ready(
            vec![LoopInput {
                text: "mixed".to_string(),
                input_id: None,
                images: Vec::new(),
                accepted: None,
            }],
            DrainEpoch(0),
        ));
        drain_q.push_back(DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        });

        let mut port = ScriptedScenario {
            model_steps: VecDeque::from([ModelStep::Tools {
                text: String::new(),
                calls: vec![bash_call.clone(), question_call.clone()],
            }]),
            tool_steps: VecDeque::from([ToolStep::InteractionSuspended {
                completed_results: vec![(bash_id.clone(), ToolCallStatus::Success)],
                fuse_bypassed: Vec::new(),
                suspended: vec![suspended_q],
            }]),
            drain_outcomes: drain_q,
            ..Default::default()
        };

        // First run_loop: non-interaction → Success, suspension → AwaitUser
        let mut execution = crate::application::run::execution_state::RunExecutionState::new();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::AwaitUser);
        assert_eq!(run.status(), RunStatus::AwaitingUser);

        // Bash call already Success (advanced by engine before interaction)
        let step = &run.steps()[0];
        let bash_tc = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &bash_id)
            .unwrap();
        assert_eq!(
            bash_tc.status(),
            ToolCallStatus::Success,
            "non-interaction call should be Success after first run"
        );

        // Reply to the question via bridge
        let request_id = execution
            .interaction_metadata()
            .first()
            .expect("should have stored metadata")
            .request_id
            .clone();
        let reply = sdk::InteractionReply::UserQuestions(vec![sdk::UserAnswer("yes".to_string())]);
        assert_eq!(
            port.interaction_bridge.reply(&request_id, reply),
            sdk::InteractionCommandOutcome::Accepted
        );

        // Second run_loop: resolve suspension → complete step
        port.drain_outcomes = VecDeque::from([DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(1),
        }]);
        port.sync_inputs();
        let directive = run_loop(
            &mut run,
            &mut execution,
            &cancel,
            &mut scripted_run_loop(&mut port),
        )
        .await
        .unwrap();
        assert_eq!(directive, LoopDirective::Terminal);
        assert_eq!(run.status(), RunStatus::Completed);

        // Both calls are Success; bash was NOT re-advanced
        let step = &run.steps()[0];
        let bash_tc = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &bash_id)
            .unwrap();
        let question_tc = step
            .tool_calls()
            .iter()
            .find(|tc| tc.id() == &question_id)
            .unwrap();
        assert_eq!(bash_tc.status(), ToolCallStatus::Success);
        assert_eq!(question_tc.status(), ToolCallStatus::Success);
    }
}
