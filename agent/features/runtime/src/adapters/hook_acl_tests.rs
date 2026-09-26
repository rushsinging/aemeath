//! 测试先行：Runtime application hook adapter —— 由 `hook::HookOutcomeData`
//! 到 Runtime-owned `RuntimeHookDispatch` 的纯值投影。
//!
//! 这些测试固定以下不变式（对应 #925）：
//! - directive 完整覆盖 Continue / Block(reason) / Context / UpdatedInput / ContextAndInput；
//! - `RuntimeHookReason` 结构化对应 `hook::HookReasonData` 的全部 variant，
//!   绝不压成 Debug 字符串；
//! - execution 完整保留 status / attempts / exit_code / stdout / stderr / duration；
//! - messages（#925 BC 展示消息）按源顺序 1:1 投影，point / source /
//!   execution_ordinal / attempt / kind / text 全部保留，不合并、不丢失顺序；
//! - 不解析 stdout / JSON（仅原样搬运），不维护 Run 状态。

use std::time::Duration;

use hook::{
    HookDirectiveData, HookDisplayMessageData, HookDisplayMessageKindData, HookExecutionData,
    HookExecutionStatusData, HookOutcomeData, HookPointData, HookReasonData,
};
use serde_json::json;

use super::hook_acl::{
    map_hook_outcome, RuntimeHookDirective, RuntimeHookDispatch, RuntimeHookDisplayMessageKind,
    RuntimeHookExecution, RuntimeHookExecutionStatus, RuntimeHookReason,
};

// ─── helpers ──────────────────────────────────────────────────

fn exec(
    status: HookExecutionStatusData,
    attempts: u8,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    duration: Duration,
) -> HookExecutionData {
    HookExecutionData {
        status,
        attempts,
        exit_code,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
        stdout_file: None,
        stderr_file: None,
        duration,
    }
}

/// 构造一个 `messages` 为空的 `HookOutcomeData`，避免每个测试 literal 重复写
/// `messages: Vec::new()`（#925 之后 `HookOutcomeData` 多了 `messages` 字段）。
fn outcome(directive: HookDirectiveData, executions: Vec<HookExecutionData>) -> HookOutcomeData {
    HookOutcomeData {
        executions,
        directive,
        messages: Vec::new(),
        block_detail: None,
    }
}

/// 构造一条 `hook::HookDisplayMessageData` 测试夹具，减少新测试里的字段重复。
fn msg(
    point: HookPointData,
    source: &str,
    execution_ordinal: u32,
    attempt: u8,
    kind: HookDisplayMessageKindData,
    text: &str,
) -> HookDisplayMessageData {
    HookDisplayMessageData {
        point,
        source: source.to_string(),
        execution_ordinal,
        attempt,
        kind,
        text: text.to_string(),
    }
}

// ─── Continue ─────────────────────────────────────────────────

#[test]
fn continue_directive_projects_with_no_executions() {
    let outcome = HookOutcomeData::proceed();
    let dispatch = map_hook_outcome(&outcome);

    assert_eq!(dispatch.directive, RuntimeHookDirective::Continue);
    assert!(dispatch.executions.is_empty());
    assert!(dispatch.messages.is_empty());
}

// ─── Block × every HookReasonData variant (structured, not Debug string) ──

#[test]
fn block_exit_code_reason_preserves_code_and_stderr() {
    let outcome = outcome(
        HookDirectiveData::Block {
            reason: HookReasonData::ExitCode {
                code: 2,
                stderr: "boom".to_string(),
            },
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::Block {
            reason: RuntimeHookReason::ExitCode { code, stderr },
        } => {
            assert_eq!(code, 2);
            assert_eq!(stderr, "boom");
        }
        other => panic!("expected Block(ExitCode), got {other:?}"),
    }
}

#[test]
fn block_exit_code_reason_with_empty_stderr_preserved() {
    let outcome = outcome(
        HookDirectiveData::Block {
            reason: HookReasonData::ExitCode {
                code: 137,
                stderr: String::new(),
            },
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::Block {
            reason: RuntimeHookReason::ExitCode { code, stderr },
        } => {
            assert_eq!(code, 137);
            assert!(stderr.is_empty());
        }
        other => panic!("expected Block(ExitCode) with empty stderr, got {other:?}"),
    }
}

#[test]
fn block_json_block_reason_preserves_reason() {
    let outcome = outcome(
        HookDirectiveData::Block {
            reason: HookReasonData::JsonBlock {
                reason: "forbidden by policy".to_string(),
            },
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::Block {
            reason: RuntimeHookReason::JsonBlock { reason },
        } => assert_eq!(reason, "forbidden by policy"),
        other => panic!("expected Block(JsonBlock), got {other:?}"),
    }
}

#[test]
fn block_json_continue_false_preserves_stop_reason_some() {
    let outcome = outcome(
        HookDirectiveData::Block {
            reason: HookReasonData::JsonContinueFalse {
                stop_reason: Some("end_turn".to_string()),
            },
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::Block {
            reason: RuntimeHookReason::JsonContinueFalse { stop_reason },
        } => assert_eq!(stop_reason.as_deref(), Some("end_turn")),
        other => panic!("expected Block(JsonContinueFalse), got {other:?}"),
    }
}

#[test]
fn block_json_continue_false_preserves_stop_reason_none() {
    let outcome = outcome(
        HookDirectiveData::Block {
            reason: HookReasonData::JsonContinueFalse { stop_reason: None },
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::Block {
            reason: RuntimeHookReason::JsonContinueFalse { stop_reason },
        } => assert!(stop_reason.is_none()),
        other => panic!("expected Block(JsonContinueFalse), got {other:?}"),
    }
}

#[test]
fn block_stop_hook_execution_failed_preserves_error() {
    let outcome = outcome(
        HookDirectiveData::Block {
            reason: HookReasonData::StopHookExecutionFailed {
                error: "retry exhausted".to_string(),
            },
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::Block {
            reason: RuntimeHookReason::StopHookExecutionFailed { error },
        } => assert_eq!(error, "retry exhausted"),
        other => panic!("expected Block(StopHookExecutionFailed), got {other:?}"),
    }
}

#[test]
fn block_policy_block_preserves_error() {
    let outcome = outcome(
        HookDirectiveData::Block {
            reason: HookReasonData::PolicyBlock {
                error: "policy=block".to_string(),
            },
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::Block {
            reason: RuntimeHookReason::PolicyBlock { error },
        } => assert_eq!(error, "policy=block"),
        other => panic!("expected Block(PolicyBlock), got {other:?}"),
    }
}

/// 关键不变式：`HookReasonData` 不得压成仅 Debug 字符串——即便两个 reason
/// 共享相同文本（"same"），只要 variant 不同，投影后也必须可区分。
#[test]
fn reason_is_structural_not_flattened_to_debug_string() {
    let mk = |reason: HookReasonData| {
        map_hook_outcome(&outcome(HookDirectiveData::Block { reason }, Vec::new())).directive
    };

    // JsonBlock.reason = "same" 与 StopHookExecutionFailed.error = "same" 文本相同，
    // 若压成 Debug 字符串将无法区分；结构化投影必须保留 variant 边界。
    let json_block = mk(HookReasonData::JsonBlock {
        reason: "same".to_string(),
    });
    let stop_failed = mk(HookReasonData::StopHookExecutionFailed {
        error: "same".to_string(),
    });

    assert_ne!(json_block, stop_failed);
    assert!(matches!(
        json_block,
        RuntimeHookDirective::Block {
            reason: RuntimeHookReason::JsonBlock { .. }
        }
    ));
    assert!(matches!(
        stop_failed,
        RuntimeHookDirective::Block {
            reason: RuntimeHookReason::StopHookExecutionFailed { .. }
        }
    ));
}

/// 覆盖性：5 个 reason variant 全部映射到对应的 Runtime variant。
#[test]
fn all_hook_reason_variants_have_a_runtime_counterpart() {
    let reasons = [
        RuntimeHookReason::ExitCode {
            code: 1,
            stderr: "e".to_string(),
        },
        RuntimeHookReason::JsonBlock {
            reason: "r".to_string(),
        },
        RuntimeHookReason::JsonContinueFalse {
            stop_reason: Some("s".to_string()),
        },
        RuntimeHookReason::JsonContinueFalse { stop_reason: None },
        RuntimeHookReason::StopHookExecutionFailed {
            error: "x".to_string(),
        },
        RuntimeHookReason::PolicyBlock {
            error: "p".to_string(),
        },
    ];
    // 不同 variant 之间互不相等（结构化保留）。
    for (i, a) in reasons.iter().enumerate() {
        for (j, b) in reasons.iter().enumerate() {
            if i == j {
                assert_eq!(a, b, "identical entries must be equal at {i}");
            } else {
                assert_ne!(a, b, "distinct variants must differ at ({i},{j})");
            }
        }
    }
}

// ─── Context / UpdatedInput / ContextAndInput ─────────────────

#[test]
fn context_directive_preserves_context_string() {
    let outcome = outcome(
        HookDirectiveData::ContinueWithContext {
            context: "extra guidance".to_string(),
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::Context { context } => assert_eq!(context, "extra guidance"),
        other => panic!("expected Context, got {other:?}"),
    }
}

/// UpdatedInput 携带的是 hook BC 已解析的 `serde_json::Value`；adapter 仅搬运，
/// 不再解析 stdout / JSON。
#[test]
fn updated_input_directive_preserves_json_value() {
    let value = json!({"decision": "block", "reason": "no"});
    let outcome = outcome(
        HookDirectiveData::ContinueWithUpdatedInput {
            input: value.clone(),
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::UpdatedInput { input } => assert_eq!(input, value),
        other => panic!("expected UpdatedInput, got {other:?}"),
    }
}

#[test]
fn context_and_input_directive_preserves_both_fields() {
    let value = json!({"k": 42});
    let outcome = outcome(
        HookDirectiveData::ContinueWithContextAndInput {
            context: "ctx".to_string(),
            input: value.clone(),
        },
        Vec::new(),
    );
    let dispatch = map_hook_outcome(&outcome);

    match dispatch.directive {
        RuntimeHookDirective::ContextAndInput { context, input } => {
            assert_eq!(context, "ctx");
            assert_eq!(input, value);
        }
        other => panic!("expected ContextAndInput, got {other:?}"),
    }
}

// ─── execution field preservation ─────────────────────────────

#[test]
fn execution_success_preserves_all_fields() {
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![exec(
            HookExecutionStatusData::Success,
            1,
            Some(0),
            "{\"ok\":true}",
            "",
            Duration::from_millis(123),
        )],
    );
    let dispatch = map_hook_outcome(&outcome);

    let exec = &dispatch.executions[0];
    assert_eq!(exec.status, RuntimeHookExecutionStatus::Success);
    assert_eq!(exec.attempts, 1);
    assert_eq!(exec.exit_code, Some(0));
    assert_eq!(exec.stdout, "{\"ok\":true}");
    assert_eq!(exec.stderr, "");
    assert_eq!(exec.duration, Duration::from_millis(123));
}

#[test]
fn execution_blocked_status_preserved() {
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![exec(
            HookExecutionStatusData::Blocked,
            1,
            Some(2),
            "",
            "denied",
            Duration::from_millis(5),
        )],
    );
    let dispatch = map_hook_outcome(&outcome);

    assert_eq!(
        dispatch.executions[0].status,
        RuntimeHookExecutionStatus::Blocked
    );
}

#[test]
fn execution_cancelled_preserves_typed_status() {
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![exec(
            HookExecutionStatusData::Cancelled,
            1,
            None,
            "",
            "",
            Duration::ZERO,
        )],
    );

    let dispatch = map_hook_outcome(&outcome);

    assert!(matches!(
        dispatch.executions[0].status,
        RuntimeHookExecutionStatus::Cancelled
    ));
}

#[test]
fn execution_failed_preserves_error_message() {
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![exec(
            HookExecutionStatusData::ExecutionFailed {
                error: "spawn failed".to_string(),
            },
            3,
            None,
            "",
            "",
            Duration::from_millis(9),
        )],
    );
    let dispatch = map_hook_outcome(&outcome);

    match &dispatch.executions[0].status {
        RuntimeHookExecutionStatus::ExecutionFailed { error } => {
            assert_eq!(error, "spawn failed");
        }
        other => panic!("expected ExecutionFailed, got {other:?}"),
    }
    assert_eq!(dispatch.executions[0].attempts, 3);
    assert_eq!(dispatch.executions[0].exit_code, None);
}

#[test]
fn execution_missing_exit_code_preserved_as_none() {
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![exec(
            HookExecutionStatusData::ExecutionFailed {
                error: "timeout".to_string(),
            },
            2,
            None,
            "partial",
            "",
            Duration::from_secs(1),
        )],
    );
    let dispatch = map_hook_outcome(&outcome);

    assert_eq!(dispatch.executions[0].exit_code, None);
    assert_eq!(dispatch.executions[0].stdout, "partial");
}

#[test]
fn duration_preserved_exactly() {
    let dur = Duration::new(7, 123_456);
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![exec(
            HookExecutionStatusData::Success,
            1,
            Some(0),
            "",
            "",
            dur,
        )],
    );
    let dispatch = map_hook_outcome(&outcome);

    assert_eq!(dispatch.executions[0].duration, dur);
}

/// stdout / stderr 必须原样搬运，不得被当作 JSON 解析、截断或改写。
#[test]
fn stdout_and_stderr_preserved_verbatim_without_parsing() {
    // 看起来像 JSON 的 stdout 不应被解析；非法 JSON 的 stdout 也不应导致失败。
    let raw = "{ this is not valid json ]]}}";
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![exec(
            HookExecutionStatusData::Success,
            1,
            Some(0),
            raw,
            "stderr line\nsecond line",
            Duration::from_micros(42),
        )],
    );
    let dispatch = map_hook_outcome(&outcome);

    assert_eq!(dispatch.executions[0].stdout, raw);
    assert_eq!(dispatch.executions[0].stderr, "stderr line\nsecond line");
}

// ─── ordering & retry trajectory ──────────────────────────────

#[test]
fn multiple_executions_preserved_in_order() {
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![
            exec(
                HookExecutionStatusData::Success,
                1,
                Some(0),
                "first",
                "",
                Duration::from_millis(1),
            ),
            exec(
                HookExecutionStatusData::Blocked,
                1,
                Some(2),
                "second",
                "b",
                Duration::from_millis(2),
            ),
        ],
    );
    let dispatch = map_hook_outcome(&outcome);

    assert_eq!(dispatch.executions.len(), 2);
    assert_eq!(dispatch.executions[0].stdout, "first");
    assert_eq!(dispatch.executions[1].stdout, "second");
    assert_eq!(
        dispatch.executions[0].status,
        RuntimeHookExecutionStatus::Success
    );
    assert_eq!(
        dispatch.executions[1].status,
        RuntimeHookExecutionStatus::Blocked
    );
}

/// 重试轨迹必须完整保留：两次 ExecutionFailed 后第三次成功，
/// `executions` 必须包含全部三次（含失败 attempt），不得丢弃重试历史。
#[test]
fn retry_trajectory_preserved_with_three_attempts() {
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![
            exec(
                HookExecutionStatusData::ExecutionFailed {
                    error: "busy".to_string(),
                },
                1,
                None,
                "",
                "",
                Duration::from_millis(10),
            ),
            exec(
                HookExecutionStatusData::ExecutionFailed {
                    error: "busy".to_string(),
                },
                2,
                None,
                "",
                "",
                Duration::from_millis(10),
            ),
            exec(
                HookExecutionStatusData::Success,
                3,
                Some(0),
                "ok",
                "",
                Duration::from_millis(10),
            ),
        ],
    );
    let dispatch = map_hook_outcome(&outcome);

    assert_eq!(dispatch.executions.len(), 3);
    assert!(matches!(
        dispatch.executions[0].status,
        RuntimeHookExecutionStatus::ExecutionFailed { .. }
    ));
    assert!(matches!(
        dispatch.executions[1].status,
        RuntimeHookExecutionStatus::ExecutionFailed { .. }
    ));
    assert_eq!(
        dispatch.executions[2].status,
        RuntimeHookExecutionStatus::Success
    );
    assert_eq!(dispatch.executions[2].attempts, 3);
}

// ─── messages projection（#925 BC 展示消息）────────────────────

/// 两种 message kind（AdditionalContext / SystemMessage）都映射到对应 Runtime
/// variant，且 point / source / execution_ordinal / attempt / kind / text
/// 六个字段全部 1:1 投影。
#[test]
fn messages_project_both_kinds_with_all_fields_preserved() {
    let outcome = HookOutcomeData {
        executions: Vec::new(),
        directive: HookDirectiveData::ContinueWithContext {
            context: "agg".to_string(),
        },
        messages: vec![
            msg(
                HookPointData::PreToolUse,
                "*",
                1,
                1,
                HookDisplayMessageKindData::AdditionalContext,
                "ctx-a",
            ),
            msg(
                HookPointData::Stop,
                "Write",
                2,
                3,
                HookDisplayMessageKindData::SystemMessage,
                "warn-b",
            ),
        ],
        block_detail: None,
    };
    let dispatch = map_hook_outcome(&outcome);

    assert_eq!(dispatch.messages.len(), 2);

    // 第一条：AdditionalContext，全部字段 1:1 投影。
    let m0 = &dispatch.messages[0];
    assert_eq!(m0.point, HookPointData::PreToolUse);
    assert_eq!(m0.source, "*");
    assert_eq!(m0.execution_ordinal, 1);
    assert_eq!(m0.attempt, 1);
    assert_eq!(m0.kind, RuntimeHookDisplayMessageKind::AdditionalContext);
    assert_eq!(m0.text, "ctx-a");

    // 第二条：SystemMessage；attempt=3（含重试）与 execution_ordinal=2 均原样保留。
    let m1 = &dispatch.messages[1];
    assert_eq!(m1.point, HookPointData::Stop);
    assert_eq!(m1.source, "Write");
    assert_eq!(m1.execution_ordinal, 2);
    assert_eq!(m1.attempt, 3);
    assert_eq!(m1.kind, RuntimeHookDisplayMessageKind::SystemMessage);
    assert_eq!(m1.text, "warn-b");
}

/// messages 必须按源顺序 1:1 保留：跨不同 execution_ordinal / attempt / kind
/// 交错排列的多条消息，投影后顺序与内容均不得丢失或重排（不合并、不丢弃来源）。
#[test]
fn messages_preserve_order_verbatim_no_merge_or_drop() {
    let outcome = HookOutcomeData {
        executions: Vec::new(),
        directive: HookDirectiveData::Continue,
        messages: vec![
            msg(
                HookPointData::PreToolUse,
                "*",
                1,
                1,
                HookDisplayMessageKindData::AdditionalContext,
                "m1",
            ),
            msg(
                HookPointData::PostToolUse,
                "Read",
                1,
                1,
                HookDisplayMessageKindData::SystemMessage,
                "m2",
            ),
            msg(
                HookPointData::Stop,
                "Edit",
                3,
                2,
                HookDisplayMessageKindData::AdditionalContext,
                "m3",
            ),
            msg(
                HookPointData::Stop,
                "Write",
                3,
                2,
                HookDisplayMessageKindData::SystemMessage,
                "m4",
            ),
        ],
        block_detail: None,
    };
    let dispatch = map_hook_outcome(&outcome);

    // 顺序无损：text 序列与源一致。
    let texts: Vec<&str> = dispatch.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, vec!["m1", "m2", "m3", "m4"]);

    // 来源 / 序号 / kind 严格对应（未被合并或重排）。
    assert_eq!(dispatch.messages[0].source, "*");
    assert_eq!(dispatch.messages[1].source, "Read");
    assert_eq!(dispatch.messages[2].source, "Edit");
    assert_eq!(dispatch.messages[3].source, "Write");
    assert_eq!(dispatch.messages[2].execution_ordinal, 3);
    assert_eq!(dispatch.messages[2].attempt, 2);
    assert_eq!(
        dispatch.messages[3].kind,
        RuntimeHookDisplayMessageKind::SystemMessage
    );
}

/// 空源 messages 投影为空（继续 / proceed 路径不产生展示消息）。
#[test]
fn messages_empty_when_source_has_none() {
    let outcome = outcome(HookDirectiveData::Continue, Vec::new());
    let dispatch = map_hook_outcome(&outcome);

    assert!(dispatch.messages.is_empty());
}

// ─── purity: source untouched; From impl ─────────────────────

#[test]
fn mapping_does_not_mutate_source() {
    let outcome = outcome(
        HookDirectiveData::Block {
            reason: HookReasonData::JsonBlock {
                reason: "x".to_string(),
            },
        },
        vec![exec(
            HookExecutionStatusData::Success,
            1,
            Some(0),
            "src",
            "err",
            Duration::from_millis(1),
        )],
    );
    let snapshot_directive = format!("{:?}", outcome.directive);
    let snapshot_exec_stdout = outcome.executions[0].stdout.clone();

    let _ = map_hook_outcome(&outcome);

    // 源 HookOutcomeData 不受投影影响（纯函数）。
    assert_eq!(format!("{:?}", outcome.directive), snapshot_directive);
    assert_eq!(outcome.executions[0].stdout, snapshot_exec_stdout);
    assert_eq!(outcome.executions.len(), 1);
}

#[test]
fn from_impl_delegates_to_map_hook_outcome() {
    let outcome = outcome(
        HookDirectiveData::Continue,
        vec![exec(
            HookExecutionStatusData::Success,
            1,
            Some(0),
            "",
            "",
            Duration::ZERO,
        )],
    );

    let via_from: RuntimeHookDispatch = (&outcome).into();
    let via_fn = map_hook_outcome(&outcome);

    assert_eq!(via_from, via_fn);
}

#[test]
fn dispatch_round_trips_directive_and_executions_together() {
    let outcome = outcome(
        HookDirectiveData::ContinueWithContextAndInput {
            context: "c".to_string(),
            input: json!({"a": 1}),
        },
        vec![
            exec(
                HookExecutionStatusData::Blocked,
                1,
                Some(2),
                "",
                "no",
                Duration::from_millis(3),
            ),
            exec(
                HookExecutionStatusData::Success,
                2,
                Some(0),
                "{}",
                "",
                Duration::from_millis(4),
            ),
        ],
    );
    let dispatch = map_hook_outcome(&outcome);

    assert_eq!(
        dispatch,
        RuntimeHookDispatch {
            executions: vec![
                RuntimeHookExecution {
                    status: RuntimeHookExecutionStatus::Blocked,
                    attempts: 1,
                    exit_code: Some(2),
                    stdout: String::new(),
                    stderr: "no".to_string(),
                    stdout_file: None,
                    stderr_file: None,
                    duration: Duration::from_millis(3),
                },
                RuntimeHookExecution {
                    status: RuntimeHookExecutionStatus::Success,
                    attempts: 2,
                    exit_code: Some(0),
                    stdout: "{}".to_string(),
                    stderr: String::new(),
                    stdout_file: None,
                    stderr_file: None,
                    duration: Duration::from_millis(4),
                },
            ],
            directive: RuntimeHookDirective::ContextAndInput {
                context: "c".to_string(),
                input: json!({"a": 1}),
            },
            messages: Vec::new(),
            block_detail: None,
        }
    );
}
