#[tokio::test]
async fn cancel_step_during_compaction_finalizes_then_returns_to_drain() {
    let mut run = new_run(Duration::ZERO);
    let root = CancellationToken::new();
    let mut port = ScriptedScenario {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        cancel_when_compact_starts: true,
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
    assert_eq!(port.cancelled_steps(), port.frozen_steps());
    assert!(port.calls().contains(&"compact"));
    assert!(!port.calls().contains(&"model"));
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
async fn engine_terminates_in_flight_compaction_and_emits_terminal_ack() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        needs_compaction: true,
        block_compact_until_cancelled: true,
        ..Default::default()
    };
    let cancel_for_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::task::yield_now().await;
        cancel_for_task.cancel();
    });

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();
    canceller.await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(port.calls().contains(&"compact"));
    assert!(port
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::Terminated { .. })));
    assert!(!port.calls().contains(&"model"));
}

#[tokio::test]
async fn engine_terminates_in_flight_model_and_emits_terminal_ack() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        cancelled_during_model: true,
        ..Default::default()
    };
    let cancel_for_task = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::task::yield_now().await;
        cancel_for_task.cancel();
    });

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();
    canceller.await.unwrap();

    assert_eq!(directive, LoopDirective::Terminal);
    assert_eq!(run.status(), RunStatus::Terminated);
    assert_eq!(port.cancelled_steps(), port.frozen_steps());
    assert!(port
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::TerminationRequested { .. })));
    assert!(port
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::Terminated { .. })));
}

#[tokio::test]
async fn engine_passes_soft_block_decision_to_the_single_tool_adapter() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let repeated = call("Read", json!({"file_path": "a.rs"}));
    let mut port = ScriptedScenario {
        drain_outcomes: VecDeque::from([
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "one".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
                }],
                DrainEpoch(0),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "two".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
                }],
                DrainEpoch(1),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "three".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
                }],
                DrainEpoch(2),
            ),
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "four".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
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

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert_eq!(port.guarded_calls().len(), 3);
    assert_eq!(port.guarded_calls()[0], vec![ToolGuardDecision::Allow]);
    assert_eq!(port.guarded_calls()[1], vec![ToolGuardDecision::Allow]);
    assert!(matches!(
        port.guarded_calls()[2].as_slice(),
        [ToolGuardDecision::SoftBlock { .. }]
    ));
}

#[tokio::test]
async fn engine_timeout_interrupts_a_blocked_model_call() {
    let mut run = new_run(Duration::from_millis(10));
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        require_model_cancellation_cleanup: true,
        ..Default::default()
    };

    tokio::time::timeout(
        Duration::from_secs(1),
        run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &cancel,
            &mut scripted_run_loop(&mut port),
        ),
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
    let mut port = ScriptedScenario {
        model_steps: VecDeque::from([ModelStep::Tools {
            text: "question".to_string(),
            calls: vec![call("AskUserQuestion", json!({}))],
        }]),
        tool_steps: VecDeque::from([ToolStep::AwaitUser]),
        ..Default::default()
    };
    assert_eq!(
        run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &cancel,
            &mut scripted_run_loop(&mut port)
        )
        .await
        .unwrap(),
        LoopDirective::AwaitUser
    );
    let model_calls = port.calls().iter().filter(|call| **call == "model").count();

    assert_eq!(
        run_loop(
            &mut run,
            &mut crate::application::run::execution_state::RunExecutionState::new(),
            &cancel,
            &mut scripted_run_loop(&mut port)
        )
        .await
        .unwrap(),
        LoopDirective::AwaitUser
    );
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert_eq!(
        port.calls().iter().filter(|call| **call == "model").count(),
        model_calls
    );
}

#[tokio::test]
async fn failed_event_delivery_is_restored_to_the_run_outbox() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        fail_emit_once: true,
        ..Default::default()
    };

    let error = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, LoopEngineError::Adapter(_)));
    assert!(matches!(
        run.events(),
        [
            RuntimeLifecycleEvent::Transitioned { .. },
            RuntimeLifecycleEvent::Started { .. },
            RuntimeLifecycleEvent::DrainingInput { .. }
        ]
    ));
}

#[tokio::test]
async fn engine_timeout_fails_before_starting_new_work() {
    let mut run = new_run(Duration::from_nanos(1));
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario::default();

    tokio::time::sleep(Duration::from_millis(1)).await;
    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Failed);
    assert!(!port.calls().contains(&"model"));
}

// ── #1272 Drain outcome tests ──────────────────────────────────────────

/// InternalContinuation with ToolResults kind processes like user input
/// but uses DrainInternalContinuation transition (not DrainInputs).
#[tokio::test]
async fn engine_processes_internal_continuation() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
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

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    // drain_input + freeze + accept + compaction check + emit + model + finalize + emit
    assert!(port.calls().contains(&"freeze_step"));
    assert!(port.calls().contains(&"model"));
    assert!(port
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::Completed { .. })));
}

/// #1272: InternalContinuation with empty batch while AwaitingUser
/// must NOT auto-resume. The engine returns AwaitUser;
/// only Ready (guaranteed non-empty) resumes from AwaitingUser.
#[tokio::test]
async fn internal_continuation_while_awaiting_user_without_input_stays_awaiting() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    // First call: model → Tools → AwaitUser → EmptyAndSealed → AwaitUser
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
    let calls_before_second_loop = port.calls().len();

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
    port.sync_inputs();

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();
    assert_eq!(
        directive,
        LoopDirective::AwaitUser,
        "InternalContinuation with empty batch must NOT resume from AwaitingUser"
    );
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    // Only drain was called (no step processing). When AwaitingUser,
    // the engine calls await_user_input, which pushes "await_input".
    assert_eq!(
        port.calls().len(),
        calls_before_second_loop + 1,
        "Only one drain call should have been made, not step processing"
    );
    assert!(
        port.calls().last() == Some(&"await_input") || port.calls().last() == Some(&"input"),
        "Last call should be a drain call"
    );
}

/// #1272: InternalContinuation with user input while AwaitingUser
/// DOES resume — the batch carries the user's response.
#[tokio::test]
async fn internal_continuation_while_awaiting_user_with_input_resumes() {
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
    let calls_before = port.calls().len();

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
                accepted: None,
            }],
            epoch: DrainEpoch(1),
        },
        DrainOutcome::EmptyAndSealed {
            epoch: DrainEpoch(2),
        },
    ]);
    port.sync_inputs();

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();
    assert_eq!(run.status(), RunStatus::Completed);
    // New calls were made (step frozen, model invoked, etc.)
    assert!(
        port.calls().len() > calls_before,
        "Should have made new calls after resuming"
    );
    assert!(port.calls().contains(&"freeze_step"));
    assert!(port.calls().contains(&"model"));
}

// ── #1272 terminal text persistence ──────────────────────────────────

/// The last assistant text before EmptyAndSealed MUST be carried in the
/// Completed event.  Previously `terminal_text` was reset to None at
/// the top of each loop iteration, so Complete→EmptyAndSealed lost it.
#[tokio::test]
async fn engine_completed_event_carries_last_assistant_text() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "final answer".to_string(),
        }]),
        ..Default::default()
    };

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    // The Completed event must carry the assistant text from the model step.
    let completed = port
        .events()
        .iter()
        .find_map(|event| match event {
            RuntimeLifecycleEvent::Completed { result, .. } => Some(result.clone()),
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
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "second".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
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

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    let completed = port
        .events()
        .iter()
        .find_map(|event| match event {
            RuntimeLifecycleEvent::Completed { result, .. } => Some(result.clone()),
            _ => None,
        })
        .expect("Completed event must be emitted");
    assert_eq!(
        completed, "now done",
        "Completed.result must be the LAST assistant text, not the first"
    );
}

// ── #1272 epoch validation tests ─────────────────────────────────────
