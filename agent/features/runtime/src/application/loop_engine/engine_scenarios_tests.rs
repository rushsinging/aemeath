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

// #1248 TaskData 6: Stop hook block counting moved to Run domain.
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
        .any(|event| matches!(event, RuntimeLifecycleEvent::Completed { .. })));
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

/// HardPause 场景的共享 drain 脚本：5 次输入各驱动一个 step（tool call fuse
/// 与重复文本均需 5 次重复才升级 HardPause），第 5 个 step 挂起期间消费
/// `NoInput` 让第一段 `run_loop` 返回 `AwaitUser`，恢复收口到 `DrainingInput`
/// 后以 `EmptyAndSealed` 封口。`awaits_user_before_seal` 为 `false` 时省略
/// `NoInput`（守卫降级、无挂起的场景）。
fn five_step_hard_pause_drain_script(awaits_user_before_seal: bool) -> VecDeque<DrainOutcome> {
    let mut drain_outcomes: VecDeque<DrainOutcome> = (0..5)
        .map(|epoch| {
            DrainOutcome::ready(
                vec![LoopInput {
                    text: "next".to_string(),
                    input_id: None,
                    images: Vec::new(),
                    accepted: None,
                }],
                DrainEpoch(epoch),
            )
        })
        .collect();
    if awaits_user_before_seal {
        drain_outcomes.push_back(DrainOutcome::NoInput {
            epoch: DrainEpoch(5),
        });
    }
    drain_outcomes.push_back(DrainOutcome::EmptyAndSealed {
        epoch: DrainEpoch(5),
    });
    drain_outcomes
}

/// 断点 A 场景复现：连续 5 个 step 返回同一工具调用使 tool call fuse 升级
/// HardPause。挂起点处于 `AwaitingToolApproval`（`ResponseWithTools` 之后），
/// run 必须挂起为 `AwaitingUser` 并携带 `ContinueAfterHardPause`，
/// 而不是经 `fail_run` 落入 `Failed`。
#[tokio::test]
async fn tool_fuse_hard_pause_suspends_run_instead_of_failing_it() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        drain_outcomes: five_step_hard_pause_drain_script(true),
        model_steps: VecDeque::from(
            (0..5)
                .map(|step_index| ModelStep::Tools {
                    text: format!("step-{step_index}"),
                    calls: vec![call("Read", json!({"file_path": "a.rs"}))],
                })
                .collect::<Vec<_>>(),
        ),
        tool_steps: VecDeque::from(vec![ToolStep::Continue; 5]),
        ..Default::default()
    };
    let published_interactions = Arc::clone(&port.published_interactions);

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap_or_else(|error| panic!("fuse HardPause 应挂起 run 而非使其失败: {error:?}"));

    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert!(matches!(
        run.pending_interaction().map(|pending| &pending.continuation),
        Some(InteractionContinuation::ContinueAfterHardPause)
    ));
    let hard_pause_request_count = published_interactions
        .lock()
        .unwrap()
        .iter()
        .filter(|request| matches!(request.body, sdk::InteractionRequestBody::HardPause(_)))
        .count();
    assert_eq!(
        hard_pause_request_count, 1,
        "fuse HardPause 应恰好发布一次 HardPause 交互请求"
    );
}

/// 断点 C 场景复现（工具检查来源）：HardPause 挂起后回复 `HardPauseContinue`，
/// run 必须把被中断的工具轮收口到 `DrainingInput` 并继续 drain 到 `Completed`，
/// 而不是在恢复后的普通 drain 上触发 `IllegalTransition`。
#[tokio::test]
async fn tool_fuse_hard_pause_continue_resumes_round_into_completed() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        drain_outcomes: five_step_hard_pause_drain_script(true),
        model_steps: VecDeque::from(
            (0..5)
                .map(|step_index| ModelStep::Tools {
                    text: format!("step-{step_index}"),
                    calls: vec![call("Read", json!({"file_path": "a.rs"}))],
                })
                .collect::<Vec<_>>(),
        ),
        tool_steps: VecDeque::from(vec![ToolStep::Continue; 5]),
        ..Default::default()
    };
    let published_interactions = Arc::clone(&port.published_interactions);
    let mut execution =
        crate::application::run::execution_state::RunExecutionState::new();

    let suspend_result = run_loop(
        &mut run,
        &mut execution,
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await;
    assert_eq!(
        suspend_result.unwrap_or_else(|error| panic!("HardPause 应挂起而非失败: {error:?}")),
        LoopDirective::AwaitUser,
        "fuse HardPause 挂起后第一段 run_loop 应返回 AwaitUser"
    );
    assert_eq!(run.status(), RunStatus::AwaitingUser);

    let hard_pause_request_id = published_interactions
        .lock()
        .unwrap()
        .iter()
        .find(|request| matches!(request.body, sdk::InteractionRequestBody::HardPause(_)))
        .map(|request| request.id.clone())
        .expect("应已发布 HardPause 交互请求");
    assert_eq!(
        port.interaction_bridge
            .reply(&hard_pause_request_id, sdk::InteractionReply::HardPauseContinue),
        sdk::InteractionCommandOutcome::Accepted,
        "HardPauseContinue 应被 interaction bridge 接受"
    );

    run_loop(
        &mut run,
        &mut execution,
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap_or_else(|error| panic!("HardPauseContinue 恢复后 run 应继续而非失败: {error:?}"));

    assert_eq!(
        run.status(),
        RunStatus::Completed,
        "恢复后工具轮应收口到 DrainingInput 并经 EmptyAndSealed 完成"
    );
}

/// 断点 C 场景复现（text stall 来源）：重复文本使 `ModelStep::Complete` 分支
/// 升级 HardPause（挂起点状态为 `ApplyingResponse`）。回复 `HardPauseContinue`
/// 后 run 必须按原 SoftBlock 收口路径完成 step 并 drain 到 `Completed`。
#[tokio::test]
async fn repeated_text_hard_pause_continue_resumes_into_completed() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        drain_outcomes: five_step_hard_pause_drain_script(true),
        model_steps: VecDeque::from(
            (0..5)
                .map(|_| ModelStep::Complete {
                    text: "same repeated text".to_string(),
                })
                .collect::<Vec<_>>(),
        ),
        ..Default::default()
    };
    let published_interactions = Arc::clone(&port.published_interactions);
    let mut execution =
        crate::application::run::execution_state::RunExecutionState::new();

    let suspend_directive = run_loop(
        &mut run,
        &mut execution,
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap_or_else(|error| panic!("text stall HardPause 应挂起而非失败: {error:?}"));
    assert_eq!(suspend_directive, LoopDirective::AwaitUser);
    assert_eq!(run.status(), RunStatus::AwaitingUser);

    let hard_pause_request_id = published_interactions
        .lock()
        .unwrap()
        .iter()
        .find(|request| matches!(request.body, sdk::InteractionRequestBody::HardPause(_)))
        .map(|request| request.id.clone())
        .expect("text stall 应发布 HardPause 交互请求");
    assert_eq!(
        port.interaction_bridge
            .reply(&hard_pause_request_id, sdk::InteractionReply::HardPauseContinue),
        sdk::InteractionCommandOutcome::Accepted,
        "HardPauseContinue 应被 interaction bridge 接受"
    );

    run_loop(
        &mut run,
        &mut execution,
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap_or_else(|error| panic!("HardPauseContinue 恢复后 run 应继续而非失败: {error:?}"));

    assert_eq!(
        run.status(),
        RunStatus::Completed,
        "text stall HardPause 恢复后应按 ContinueAfterResponse 收口并完成"
    );
}

/// 模拟 interaction binding 不可用（如 sub-agent `Unavailable` 模式）的 port：
/// 任意 `register` 立即失败，绝不挂起。
struct UnavailableInteractionPort;

impl InteractionPort for UnavailableInteractionPort {
    fn register(
        &self,
        _request: InteractionRequest,
    ) -> Result<
        tokio::sync::oneshot::Receiver<crate::application::interaction::port::InteractionCompletion>,
        crate::application::interaction::port::InteractionPortError,
    > {
        Err(crate::application::interaction::port::InteractionPortError::Unavailable)
    }

    fn contains(&self, _request_id: &sdk::InteractionRequestId) -> bool {
        false
    }

    fn reply(
        &self,
        _request_id: &sdk::InteractionRequestId,
        _reply: sdk::InteractionReply,
    ) -> sdk::InteractionCommandOutcome {
        sdk::InteractionCommandOutcome::NotFound
    }

    fn cancel(
        &self,
        _request_id: &sdk::InteractionRequestId,
        _reason: sdk::InteractionCancelReason,
    ) -> sdk::InteractionCommandOutcome {
        sdk::InteractionCommandOutcome::NotFound
    }

    fn drain_run(&self, _run_id: &sdk::RunId, _reason: sdk::InteractionCancelReason) -> usize {
        0
    }
}

/// 断点 B 场景复现：同轮工具既有审批交互挂起（pending 未清）又触发 fuse
/// HardPause 时，`begin_interaction` 以 `InteractionAlreadyPending`（域状态
/// 错误）拒绝。守卫必须降级——run 保持审批挂起、绝不因 HardPause 而 Failed。
#[tokio::test]
async fn hard_pause_begin_rejection_degrades_instead_of_failing_run() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        drain_outcomes: five_step_hard_pause_drain_script(true),
        model_steps: VecDeque::from(
            (0..5)
                .map(|step_index| ModelStep::Tools {
                    text: format!("step-{step_index}"),
                    calls: vec![call("Read", json!({"file_path": "a.rs"}))],
                })
                .collect::<Vec<_>>(),
        ),
        tool_steps: VecDeque::from([
            ToolStep::Continue,
            ToolStep::Continue,
            ToolStep::Continue,
            ToolStep::Continue,
            ToolStep::AwaitingToolApproval {
                calls_needing_approval: vec![ApprovalRequiredCall {
                    call: call("Bash", json!({"command": "ls"})),
                    authorization: share::tools_vocab::AuthorizationContext::STANDARD,
                    reason: "requires approval".to_string(),
                    subject: "test".to_string(),
                }],
                completed_results: Vec::new(),
                fuse_bypassed: Vec::new(),
            },
        ]),
        ..Default::default()
    };
    let published_interactions = Arc::clone(&port.published_interactions);

    let directive = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await
    .unwrap_or_else(|error| panic!("守卫降级后 run 不应失败: {error:?}"));

    assert_eq!(directive, LoopDirective::AwaitUser);
    assert_eq!(
        run.status(),
        RunStatus::AwaitingUser,
        "HardPause begin 被域状态拒绝时守卫降级，run 保持审批挂起而非 Failed"
    );
    assert!(
        matches!(
            run.pending_interaction().map(|pending| &pending.continuation),
            Some(InteractionContinuation::ContinueToolApproval(_))
        ),
        "审批交互 pending 不得被降级的 HardPause 覆盖或清除"
    );
    let hard_pause_request_count = published_interactions
        .lock()
        .unwrap()
        .iter()
        .filter(|request| matches!(request.body, sdk::InteractionRequestBody::HardPause(_)))
        .count();
    assert_eq!(
        hard_pause_request_count, 0,
        "begin 被域状态拒绝后不应发布 HardPause 交互请求"
    );
}

/// 修复 2 的边界回归：port 层 `Unavailable`（如 Sub-run 无用户交互通道）时
/// HardPause **保持 fail-run 语义**——这是 `04-stuck-prevention` 中
/// "HardPause(Sub) → Failed 回传父" 的既有设计，NEVER 被降级吞掉。
#[tokio::test]
async fn hard_pause_unavailable_port_keeps_fail_run_semantics() {
    let mut run = new_run(Duration::ZERO);
    let cancel = CancellationToken::new();
    let mut port = ScriptedScenario {
        drain_outcomes: five_step_hard_pause_drain_script(false),
        model_steps: VecDeque::from(
            (0..5)
                .map(|step_index| ModelStep::Tools {
                    text: format!("step-{step_index}"),
                    calls: vec![call("Read", json!({"file_path": "a.rs"}))],
                })
                .collect::<Vec<_>>(),
        ),
        tool_steps: VecDeque::from(vec![ToolStep::Continue; 5]),
        interaction_port_override: Some(Arc::new(UnavailableInteractionPort)),
        ..Default::default()
    };

    let result = run_loop(
        &mut run,
        &mut crate::application::run::execution_state::RunExecutionState::new(),
        &cancel,
        &mut scripted_run_loop(&mut port),
    )
    .await;

    let error = result.expect_err("port Unavailable 时 HardPause 必须保持 fail-run 语义");
    assert!(
        error.to_string().contains("HardPause interaction unavailable"),
        "错误应来自 HardPause interaction 发起失败，实际: {error}"
    );
}
