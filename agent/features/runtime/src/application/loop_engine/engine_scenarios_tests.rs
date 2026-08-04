#[test]
fn stuck_guard_detects_repeated_text() {
    let mut guard = StuckGuard::new();
    assert_eq!(guard.inspect_text("same"), StuckDecision::Allow);
    assert_eq!(guard.inspect_text("same"), StuckDecision::Allow);
    assert!(matches!(
        guard.inspect_text("same"),
        StuckDecision::SoftBlock { .. }
    ));
}

#[test]
fn stuck_guard_detects_tool_loops_and_escalates() {
    let mut guard = StuckGuard::new();
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

// #1248 Task 6: Stop hook block counting moved to Run domain.
// The following test is removed because record_stop_hook_block no longer
// exists on StuckGuard. Equivalent coverage is in domain/agent_run/tests.rs
// and application/stop_hook_coordination_tests.rs.

#[tokio::test]
async fn engine_completes_text_only_run_through_the_run_fsm() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
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
    assert_eq!(port.frozen_steps().len(), 1);
    assert_eq!(port.finalized_steps(), port.frozen_steps());
    assert_eq!(run.steps()[0].id(), &port.frozen_steps()[0]);
    assert_eq!(run.steps().len(), 1);
    assert_eq!(
        run.steps()[0].invocation().unwrap().response(),
        "done",
        "the shared engine must record the model invocation in the Run aggregate"
    );
    assert_eq!(
        port.calls(),
        vec![
            "emit",
            "input",
            "freeze_step",
            "accept_step_input",
            "emit",
            "needs_compaction",
            "emit",
            "model",
            "emit",
            "emit",
            "finalize_step",
            "input",
            "emit",
        ]
    );
    assert!(port
        .events()
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Completed { .. })));
}

#[tokio::test]
async fn engine_accepts_input_before_building_context() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
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

    let accepted = port
        .calls()
        .iter()
        .position(|call| *call == "accept_step_input")
        .unwrap();
    let context = port
        .calls()
        .iter()
        .position(|call| *call == "needs_compaction")
        .unwrap();
    assert!(accepted < context);
}

#[tokio::test]
async fn engine_stops_before_context_when_accepted_input_durable_write_fails() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        fail_accept_input: true,
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

    assert_eq!(run.status(), RunStatus::Failed);
    assert!(port.calls().contains(&"accept_step_input"));
    assert!(!port.calls().contains(&"needs_compaction"));
    assert!(!port.calls().contains(&"model"));
}

#[tokio::test]
async fn engine_executes_tools_then_reenters_the_same_loop() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
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

    run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(
        port.calls().iter().filter(|call| **call == "model").count(),
        2
    );
    assert_eq!(
        port.calls().iter().filter(|call| **call == "tools").count(),
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
    let mut port = ScriptedScenario {
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
}

#[tokio::test]
async fn provider_context_too_long_compacts_then_rebuilds_before_reinvoking() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        model_steps: VecDeque::from([ModelStep::Complete {
            text: "done".to_string(),
        }]),
        model_errors: VecDeque::from([LoopEngineError::NeedsCompaction(
            "provider context too long".to_string(),
        )]),
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
    assert_eq!(
        port.calls(),
        vec![
            "emit",
            "input",
            "freeze_step",
            "accept_step_input",
            "emit",
            "needs_compaction",
            "emit",
            "model",
            "emit",
            "compact",
            "emit",
            "emit",
            "model",
            "emit",
            "emit",
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
    let mut port = ScriptedScenario {
        model_errors: VecDeque::from([
            LoopEngineError::NeedsCompaction("first".to_string()),
            LoopEngineError::NeedsCompaction("second".to_string()),
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

    assert_eq!(run.status(), RunStatus::Failed);
    assert_eq!(
        port.calls()
            .iter()
            .filter(|call| **call == "compact")
            .count(),
        1
    );
    assert_eq!(
        port.calls().iter().filter(|call| **call == "model").count(),
        2
    );
}

