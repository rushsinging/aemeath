//! 真值表与能力矩阵参数化测试。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §3 §5。
//!
//! #924 typed 协议分类：
//! - 非法 JSON → typed `InvalidJson`；
//! - 能力违规 → typed `Protocol` failure；
//! - exit 1/2/127 → `Block`（阻塞 point）/ `Protocol{BlockOnNonBlocking}`（非阻塞 point）。
//!
//! `classify_directive` 返回 `Result<HookDirective, ClassifyError>`。

#![cfg(test)]

use crate::domain::invocation::HookPoint;
use crate::domain::outcome::{ClassifyError, HookDirective, HookReason, ProtocolViolation};
use crate::domain::protocol::{classify_directive, default_true, truncate, OUTPUT_MAX_BYTES};

// ════════════════════════════════════════════════════════════
// 真值表测试
// ════════════════════════════════════════════════════════════

/// 参数化测试辅助：exit 0 + 空输出 → Continue。
#[test]
fn test_classify_exit0_empty_stdout() {
    for point in all_points() {
        let d = classify_directive(point, Some(0), "", "");
        assert!(
            matches!(d, Ok(HookDirective::Continue)),
            "{point:?}: exit 0 + 空 stdout 应返回 Ok(Continue)，实际 = {d:?}"
        );
    }
}

/// exit 0 + 空白输出 → Continue。
#[test]
fn test_classify_exit0_whitespace_stdout() {
    let d = classify_directive(HookPoint::PreToolUse, Some(0), "   \n  ", "");
    assert!(matches!(d, Ok(HookDirective::Continue)));
}

/// exit 0 + 合法 JSON（无特殊字段） → Continue。
#[test]
fn test_classify_exit0_plain_json() {
    let d = classify_directive(HookPoint::PreToolUse, Some(0), "{}", "");
    assert!(matches!(d, Ok(HookDirective::Continue)));
}

/// exit 0 + `{"continue":true}` → Continue。
#[test]
fn test_classify_exit0_json_continue_true() {
    let d = classify_directive(HookPoint::PreToolUse, Some(0), r#"{"continue": true}"#, "");
    assert!(matches!(d, Ok(HookDirective::Continue)));
}

/// #924：exit 0 + 非法 JSON → typed `InvalidJson`。
#[test]
fn test_classify_exit0_invalid_json_is_typed_invalid_json() {
    let d = classify_directive(HookPoint::PreToolUse, Some(0), "not json", "");
    assert!(
        matches!(d, Err(ClassifyError::InvalidJson { .. })),
        "非法 JSON 应分类为 typed InvalidJson，实际 = {d:?}"
    );
}

/// exit_code=None（进程未正常退出）→ typed `MissingExitCode`。
///
/// 进程未正常退出时没有退出码，必须进入 ExecutionFailed 可重试路径，
/// **不得**按空 stdout 误判为 Continue。
#[test]
fn test_classify_none_exit_code_is_missing_exit_code() {
    for point in all_points() {
        // 即使 stdout 为空也不得当 Continue。
        let d = classify_directive(point, None, "", "");
        assert!(
            matches!(d, Err(ClassifyError::MissingExitCode)),
            "{point:?}: exit_code=None 应分类为 typed MissingExitCode，实际 = {d:?}"
        );
    }
}

/// exit_code=None 即使带非空 stdout 仍为 `MissingExitCode`（退出码缺失优先于 stdout 解析）。
#[test]
fn test_classify_none_exit_code_with_stdout_still_missing() {
    let d = classify_directive(
        HookPoint::PreToolUse,
        None,
        r#"{"additionalContext":"x"}"#,
        "",
    );
    assert!(
        matches!(d, Err(ClassifyError::MissingExitCode)),
        "exit_code=None 应优先于 stdout 解析为 MissingExitCode，实际 = {d:?}"
    );
}

/// #924：非法 JSON 的 `raw` 携带触发解析失败的原始 stdout。
#[test]
fn test_classify_exit0_invalid_json_carries_raw() {
    let d = classify_directive(HookPoint::PreToolUse, Some(0), "not json", "");
    match d {
        Err(ClassifyError::InvalidJson { raw, error }) => {
            assert!(!raw.is_empty(), "raw 应携带原始 stdout");
            assert!(!error.is_empty(), "error 应携带解析错误摘要");
        }
        other => panic!("期望 Err(InvalidJson)，实际 = {other:?}"),
    }
}

/// exit 0 + `{"decision":"block"}` → Block（阻塞 point）。
#[test]
fn test_classify_exit0_json_decision_block_on_blocking_point() {
    let d = classify_directive(
        HookPoint::PreToolUse,
        Some(0),
        r#"{"decision":"block","reason":"forbidden"}"#,
        "",
    )
    .expect("阻塞 point 的 JSON block 应为 Ok");
    assert!(matches!(
        d,
        HookDirective::Block {
            reason: HookReason::JsonBlock { ref reason }
        } if reason == "forbidden"
    ));
}

/// exit 0 + `{"decision":"block"}` 无 reason 字段 → Block{reason:""}。
#[test]
fn test_classify_exit0_json_decision_block_no_reason() {
    let d = classify_directive(
        HookPoint::PreToolUse,
        Some(0),
        r#"{"decision":"block"}"#,
        "",
    )
    .expect("应为 Ok(Block)");
    assert!(matches!(
        d,
        HookDirective::Block {
            reason: HookReason::JsonBlock { ref reason }
        } if reason.is_empty()
    ));
}

/// exit 0 + `{"continue":false}` → Block（阻塞 point）。
#[test]
fn test_classify_exit0_json_continue_false_on_blocking_point() {
    let d = classify_directive(
        HookPoint::Stop,
        Some(0),
        r#"{"continue":false,"stopReason":"needs more work"}"#,
        "",
    )
    .expect("Stop 的 continue:false 应为 Ok(Block)");
    assert!(matches!(
        d,
        HookDirective::Block {
            reason: HookReason::JsonContinueFalse { ref stop_reason }
        } if stop_reason.as_deref() == Some("needs more work")
    ));
}

/// exit 0 + `{"continue":false}` 无 stopReason → Block{stop_reason:None}。
#[test]
fn test_classify_exit0_json_continue_false_no_stop_reason() {
    let d = classify_directive(HookPoint::Stop, Some(0), r#"{"continue":false}"#, "")
        .expect("应为 Ok(Block)");
    assert!(matches!(
        d,
        HookDirective::Block {
            reason: HookReason::JsonContinueFalse { stop_reason: None }
        }
    ));
}

/// #924：exit 1/2/127（任意非零）→ Block（阻塞 point）。
#[test]
fn test_classify_exit_codes_1_2_127_are_block() {
    for code in [1, 2, 127] {
        let d = classify_directive(HookPoint::PreToolUse, Some(code), "", "boom");
        match d {
            Ok(HookDirective::Block {
                reason:
                    HookReason::ExitCode {
                        code: c,
                        ref stderr,
                    },
            }) => {
                assert_eq!(c, code, "exit {code} 应映射到 ExitCode{{code:{code}}}");
                assert_eq!(stderr, "boom", "exit {code} 的 stderr 应原样保留");
            }
            other => panic!("exit {code} 在阻塞 point 应为 Ok(Block)，实际 = {other:?}"),
        }
    }
}

/// 非零 exit → Block（阻塞 point），携带 exit code 与 stderr。
#[test]
fn test_classify_nonzero_exit_on_blocking_point() {
    let d = classify_directive(HookPoint::PreToolUse, Some(1), "", "error occurred")
        .expect("阻塞 point 的非零 exit 应为 Ok(Block)");
    assert!(matches!(
        d,
        HookDirective::Block {
            reason: HookReason::ExitCode { code: 1, ref stderr }
        } if stderr == "error occurred"
    ));
}

/// 非零 exit + 空 stderr → Block{stderr:""}。
#[test]
fn test_classify_nonzero_exit_empty_stderr() {
    let d = classify_directive(HookPoint::PreToolUse, Some(2), "", "").expect("应为 Ok(Block)");
    assert!(matches!(
        d,
        HookDirective::Block {
            reason: HookReason::ExitCode { code: 2, stderr: ref s }
        } if s.is_empty()
    ));
}

/// exit 0 + additionalContext → ContinueWithContext。
#[test]
fn test_classify_exit0_additional_context() {
    let d = classify_directive(
        HookPoint::PreToolUse,
        Some(0),
        r#"{"additionalContext":"extra info"}"#,
        "",
    )
    .expect("应为 Ok(ContinueWithContext)");
    assert!(matches!(
        d,
        HookDirective::ContinueWithContext { ref context }
        if context == "extra info"
    ));
}

/// exit 0 + hookSpecificOutput.updatedInput → ContinueWithUpdatedInput。
#[test]
fn test_classify_exit0_updated_input() {
    let d = classify_directive(
        HookPoint::PreToolUse,
        Some(0),
        r#"{"hookSpecificOutput":{"updatedInput":{"command":"ls -la"}}}"#,
        "",
    )
    .expect("应为 Ok(ContinueWithUpdatedInput)");
    assert!(matches!(
        d,
        HookDirective::ContinueWithUpdatedInput { ref input }
        if input["command"] == "ls -la"
    ));
}

/// exit 0 + additionalContext + updatedInput → ContinueWithContextAndInput。
#[test]
fn test_classify_exit0_context_and_input() {
    let d = classify_directive(
        HookPoint::PreToolUse,
        Some(0),
        r#"{"additionalContext":"ctx","hookSpecificOutput":{"updatedInput":{"x":1}}}"#,
        "",
    )
    .expect("应为 Ok(ContinueWithContextAndInput)");
    assert!(matches!(
        d,
        HookDirective::ContinueWithContextAndInput { ref context, ref input }
        if context == "ctx" && input["x"] == 1
    ));
}

/// decision:block 优先于 additionalContext（先判 Block）。
#[test]
fn test_classify_block_priority_over_context() {
    let d = classify_directive(
        HookPoint::PreToolUse,
        Some(0),
        r#"{"decision":"block","reason":"denied","additionalContext":"ctx"}"#,
        "",
    )
    .expect("decision:block 应为 Ok");
    assert!(matches!(d, HookDirective::Block { .. }));
}

// ════════════════════════════════════════════════════════════
// 能力矩阵违规 → typed Protocol failure（#924）
// ════════════════════════════════════════════════════════════

/// #924：非零 exit 在非阻塞 point 上 → typed `Protocol{BlockOnNonBlocking}`。
#[test]
fn test_nonzero_exit_on_non_blocking_point_is_protocol_error() {
    for point in all_points() {
        let meta = point.metadata();
        if meta.can_block {
            continue;
        }
        let d = classify_directive(point, Some(1), "", "error");
        assert!(
            matches!(
                d,
                Err(ClassifyError::Protocol {
                    violation: ProtocolViolation::BlockOnNonBlocking
                })
            ),
            "{point:?}: 非阻塞 point 的非零 exit 应为 typed Protocol{{BlockOnNonBlocking}}，实际 = {d:?}"
        );
    }
}

/// #924：JSON decision:block 在非阻塞 point 上 → typed `Protocol{BlockOnNonBlocking}`。
#[test]
fn test_json_block_on_non_blocking_point_is_protocol_error() {
    for point in all_points() {
        let meta = point.metadata();
        if meta.can_block {
            continue;
        }
        let d = classify_directive(
            point,
            Some(0),
            r#"{"decision":"block","reason":"nope"}"#,
            "",
        );
        assert!(
            matches!(
                d,
                Err(ClassifyError::Protocol {
                    violation: ProtocolViolation::BlockOnNonBlocking
                })
            ),
            "{point:?}: 非阻塞 point 的 JSON block 应为 typed Protocol{{BlockOnNonBlocking}}，实际 = {d:?}"
        );
    }
}

/// #924：JSON continue:false 在非阻塞 point 上 → typed `Protocol{BlockOnNonBlocking}`。
#[test]
fn test_json_continue_false_on_non_blocking_point_is_protocol_error() {
    for point in all_points() {
        let meta = point.metadata();
        if meta.can_block {
            continue;
        }
        let d = classify_directive(point, Some(0), r#"{"continue":false}"#, "");
        assert!(
            matches!(
                d,
                Err(ClassifyError::Protocol {
                    violation: ProtocolViolation::BlockOnNonBlocking
                })
            ),
            "{point:?}: 非阻塞 point 的 continue:false 应为 typed Protocol{{BlockOnNonBlocking}}，实际 = {d:?}"
        );
    }
}

/// #924：can_add_context=false 的 point 收到 additionalContext
/// → typed `Protocol{ContextOnNonContextual}`。
#[test]
fn test_context_on_no_context_point_is_protocol_error() {
    let d = classify_directive(
        HookPoint::Stop,
        Some(0),
        r#"{"additionalContext":"extra"}"#,
        "",
    );
    assert!(
        matches!(
            d,
            Err(ClassifyError::Protocol {
                violation: ProtocolViolation::ContextOnNonContextual
            })
        ),
        "Stop point 不支持 context，应为 typed Protocol{{ContextOnNonContextual}}，实际 = {d:?}"
    );
}

/// #924：can_modify_input=false 的 point 收到 updatedInput
/// → typed `Protocol{UpdatedInputOnNonModifiable}`。
#[test]
fn test_updated_input_on_no_modify_point_is_protocol_error() {
    let d = classify_directive(
        HookPoint::Stop,
        Some(0),
        r#"{"hookSpecificOutput":{"updatedInput":{"x":1}}}"#,
        "",
    );
    assert!(
        matches!(
            d,
            Err(ClassifyError::Protocol {
                violation: ProtocolViolation::UpdatedInputOnNonModifiable
            })
        ),
        "Stop point 不支持 updatedInput，应为 typed Protocol{{UpdatedInputOnNonModifiable}}，实际 = {d:?}"
    );
}

/// can_add_context=true 但 can_modify_input=false 的 point（如 PreCompact）
/// 收到 additionalContext → ContinueWithContext（合法路径，应通过）。
#[test]
fn test_context_on_pre_compact_returns_context() {
    let d = classify_directive(
        HookPoint::PreCompact,
        Some(0),
        r#"{"additionalContext":"ctx"}"#,
        "",
    )
    .expect("PreCompact 支持 context，应为 Ok(ContinueWithContext)");
    assert!(matches!(d, HookDirective::ContinueWithContext { .. }));
}

/// #924：PreCompact 收到 updatedInput（can_modify_input=false）
/// → typed `Protocol{UpdatedInputOnNonModifiable}`。
#[test]
fn test_updated_input_on_pre_compact_is_protocol_error() {
    let d = classify_directive(
        HookPoint::PreCompact,
        Some(0),
        r#"{"hookSpecificOutput":{"updatedInput":{"x":1}}}"#,
        "",
    );
    assert!(
        matches!(
            d,
            Err(ClassifyError::Protocol {
                violation: ProtocolViolation::UpdatedInputOnNonModifiable
            })
        ),
        "PreCompact 不支持 updatedInput，应为 typed Protocol{{UpdatedInputOnNonModifiable}}，实际 = {d:?}"
    );
}

// ════════════════════════════════════════════════════════════
// 能力矩阵测试
// ════════════════════════════════════════════════════════════

/// 前置闸门：can_block=true。
#[test]
fn test_metadata_blocking_points() {
    let blocking_points = [
        HookPoint::PreToolUse,
        HookPoint::UserPromptSubmit,
        HookPoint::PreCompact,
        HookPoint::PermissionRequest,
        HookPoint::Elicitation,
        HookPoint::UserPromptExpansion,
        HookPoint::Stop,
    ];
    for point in blocking_points {
        let meta = point.metadata();
        assert!(meta.can_block, "{point:?} 应 can_block=true");
    }
}

/// 非 Stop 的所有 point：can_block=false。
#[test]
fn test_metadata_non_blocking_points() {
    let non_blocking = [
        HookPoint::PostToolUse,
        HookPoint::PostToolUseFailure,
        HookPoint::PostCompact,
        HookPoint::PostToolBatch,
        HookPoint::ElicitationResult,
        HookPoint::SessionStart,
        HookPoint::SessionEnd,
        HookPoint::SubRunStart,
        HookPoint::SubRunStop,
        HookPoint::TaskCreated,
        HookPoint::TaskCompleted,
        HookPoint::Notification,
        HookPoint::InstructionsLoaded,
        HookPoint::StopFailure,
        HookPoint::PermissionDenied,
        HookPoint::ConfigChange,
        HookPoint::CwdChanged,
        HookPoint::FileChanged,
        HookPoint::TeammateIdle,
    ];
    for point in non_blocking {
        let meta = point.metadata();
        assert!(!meta.can_block, "{point:?} 应 can_block=false");
    }
}

/// can_modify_input=true 的 point 只有 PreToolUse / UserPromptSubmit / PermissionRequest / Elicitation / UserPromptExpansion。
#[test]
fn test_metadata_modify_input_points() {
    let can_modify = [
        HookPoint::PreToolUse,
        HookPoint::UserPromptSubmit,
        HookPoint::PermissionRequest,
        HookPoint::Elicitation,
        HookPoint::UserPromptExpansion,
    ];
    for point in can_modify {
        let meta = point.metadata();
        assert!(meta.can_modify_input, "{point:?} 应 can_modify_input=true");
    }
}

/// Stop 的 can_modify_input=false。
#[test]
fn test_metadata_stop_no_modify_input() {
    assert!(!HookPoint::Stop.metadata().can_modify_input);
}

/// failure_policy_configurable=true 只有前置闸门（不含 Stop）。
#[test]
fn test_metadata_failure_policy_configurable() {
    let configurable = [
        HookPoint::PreToolUse,
        HookPoint::UserPromptSubmit,
        HookPoint::PreCompact,
        HookPoint::PermissionRequest,
        HookPoint::Elicitation,
        HookPoint::UserPromptExpansion,
    ];
    for point in configurable {
        assert!(
            point.metadata().failure_policy_configurable,
            "{point:?} 应 failure_policy_configurable=true"
        );
    }
    // Stop 不可配置
    assert!(!HookPoint::Stop.metadata().failure_policy_configurable);
}

/// 观察类 point 全部 false（can_block / can_add_context）。
#[test]
fn test_metadata_observation_points_all_false() {
    let observation = [
        HookPoint::StopFailure,
        HookPoint::PermissionDenied,
        HookPoint::ConfigChange,
        HookPoint::CwdChanged,
        HookPoint::FileChanged,
        HookPoint::TeammateIdle,
    ];
    for point in observation {
        let meta = point.metadata();
        assert!(!meta.can_block, "{point:?} 观察 point 应 can_block=false");
        assert!(
            !meta.can_add_context,
            "{point:?} 观察 point 应 can_add_context=false"
        );
        assert!(
            !meta.can_modify_input,
            "{point:?} 观察 point 应 can_modify_input=false"
        );
    }
}

// ════════════════════════════════════════════════════════════
// protocol 辅助函数测试（自 protocol.rs inline tests 迁移）
// ════════════════════════════════════════════════════════════

#[test]
fn test_truncate_short() {
    assert_eq!(truncate("hello"), "hello");
}

#[test]
fn test_truncate_long() {
    let long = "x".repeat(OUTPUT_MAX_BYTES + 100);
    let result = truncate(&long);
    assert!(result.len() <= OUTPUT_MAX_BYTES);
}

#[test]
fn test_default_true() {
    assert!(default_true());
}

// ════════════════════════════════════════════════════════════
// HookInvocation::point() 测试
// ════════════════════════════════════════════════════════════

/// HookInvocation::point() 对每个变体返回正确的 HookPoint。
#[test]
fn test_invocation_point_roundtrip() {
    use crate::domain::invocation::*;

    let cases: Vec<(HookInvocation, HookPoint)> = vec![
        (
            HookInvocation::PreToolUse(PreToolUseInput {
                tool_name: "Bash".into(),
                tool_input: serde_json::json!({}),
            }),
            HookPoint::PreToolUse,
        ),
        (
            HookInvocation::UserPromptSubmit(UserPromptInput {
                prompt: "hi".into(),
            }),
            HookPoint::UserPromptSubmit,
        ),
        (
            HookInvocation::PreCompact(PreCompactInput {
                turns: 1,
                messages_count: 10,
            }),
            HookPoint::PreCompact,
        ),
        (
            HookInvocation::PermissionRequest(PermissionInput {
                tool_name: "Bash".into(),
                permission_rule: "ask".into(),
            }),
            HookPoint::PermissionRequest,
        ),
        (
            HookInvocation::Elicitation(ElicitationInput {
                server_name: "srv".into(),
                elicitation_text: "text".into(),
            }),
            HookPoint::Elicitation,
        ),
        (
            HookInvocation::UserPromptExpansion(UserPromptExpansionInput {
                original_input: "a".into(),
                expanded_input: "b".into(),
            }),
            HookPoint::UserPromptExpansion,
        ),
        (
            HookInvocation::Stop(StopInput { turns: 1 }),
            HookPoint::Stop,
        ),
        (
            HookInvocation::PostToolUse(PostToolUseInput {
                tool_name: "Bash".into(),
                tool_input: serde_json::json!({}),
                tool_output: "done".into(),
                is_error: false,
            }),
            HookPoint::PostToolUse,
        ),
        (
            HookInvocation::PostToolUseFailure(PostToolUseFailureInput {
                tool_name: "Bash".into(),
                tool_input: serde_json::json!({}),
                error: "boom".into(),
            }),
            HookPoint::PostToolUseFailure,
        ),
        (
            HookInvocation::PostCompact(PostCompactInput {
                turns: 1,
                messages_before: 10,
                messages_after: 5,
            }),
            HookPoint::PostCompact,
        ),
        (
            HookInvocation::PostToolBatch(PostToolBatchInput {
                tool_count: 3,
                summary: "ok".into(),
            }),
            HookPoint::PostToolBatch,
        ),
        (
            HookInvocation::ElicitationResult(ElicitationResultInput {
                server_name: "srv".into(),
                user_response: "resp".into(),
            }),
            HookPoint::ElicitationResult,
        ),
        (
            HookInvocation::SessionStart(SessionInput {}),
            HookPoint::SessionStart,
        ),
        (
            HookInvocation::SessionEnd(SessionInput {}),
            HookPoint::SessionEnd,
        ),
        (
            HookInvocation::SubRunStart(SubRunInput {
                prompt: "p".into(),
                system: "s".into(),
                model_spec: None,
            }),
            HookPoint::SubRunStart,
        ),
        (
            HookInvocation::SubRunStop(SubRunStopInput {
                prompt: "p".into(),
                system: "s".into(),
                model_spec: None,
                result: "r".into(),
                turns: 1,
                is_error: false,
            }),
            HookPoint::SubRunStop,
        ),
        (
            HookInvocation::TaskCreated(TaskInput {
                tool_input: serde_json::json!({}),
                tool_output: "ok".into(),
            }),
            HookPoint::TaskCreated,
        ),
        (
            HookInvocation::TaskCompleted(TaskInput {
                tool_input: serde_json::json!({}),
                tool_output: "ok".into(),
            }),
            HookPoint::TaskCompleted,
        ),
        (
            HookInvocation::Notification(NotificationInput {
                notification_text: "n".into(),
                notification_type: "t".into(),
            }),
            HookPoint::Notification,
        ),
        (
            HookInvocation::InstructionsLoaded(InstructionsInput {
                file_path: "f".into(),
                instruction_type: "claude_md".into(),
            }),
            HookPoint::InstructionsLoaded,
        ),
        (
            HookInvocation::StopFailure(StopFailureInput {
                turns: 1,
                error: "e".into(),
            }),
            HookPoint::StopFailure,
        ),
        (
            HookInvocation::PermissionDenied(PermissionInput {
                tool_name: "Bash".into(),
                permission_rule: "deny".into(),
            }),
            HookPoint::PermissionDenied,
        ),
        (
            HookInvocation::ConfigChange(ConfigChangeInput {
                config_file: "f".into(),
                changed_field: None,
            }),
            HookPoint::ConfigChange,
        ),
        (
            HookInvocation::CwdChanged(CwdChangedInput {
                old_cwd: "/a".into(),
                new_cwd: "/b".into(),
            }),
            HookPoint::CwdChanged,
        ),
        (
            HookInvocation::FileChanged(FileChangedInput {
                file_path: "f".into(),
                change_type: "write".into(),
            }),
            HookPoint::FileChanged,
        ),
        (
            HookInvocation::TeammateIdle(TeammateIdleInput {
                teammate_name: "t".into(),
                idle_reason: None,
            }),
            HookPoint::TeammateIdle,
        ),
    ];

    for (inv, expected_point) in cases {
        assert_eq!(
            inv.point(),
            expected_point,
            "HookInvocation 变体的 point() 不匹配"
        );
    }
}

// ════════════════════════════════════════════════════════════
// 辅助
// ════════════════════════════════════════════════════════════

/// 返回全部 26 个 HookPoint（用于参数化测试）。
fn all_points() -> Vec<HookPoint> {
    vec![
        HookPoint::PreToolUse,
        HookPoint::UserPromptSubmit,
        HookPoint::PreCompact,
        HookPoint::PermissionRequest,
        HookPoint::Elicitation,
        HookPoint::UserPromptExpansion,
        HookPoint::Stop,
        HookPoint::PostToolUse,
        HookPoint::PostToolUseFailure,
        HookPoint::PostCompact,
        HookPoint::PostToolBatch,
        HookPoint::ElicitationResult,
        HookPoint::SessionStart,
        HookPoint::SessionEnd,
        HookPoint::SubRunStart,
        HookPoint::SubRunStop,
        HookPoint::TaskCreated,
        HookPoint::TaskCompleted,
        HookPoint::Notification,
        HookPoint::InstructionsLoaded,
        HookPoint::StopFailure,
        HookPoint::PermissionDenied,
        HookPoint::ConfigChange,
        HookPoint::CwdChanged,
        HookPoint::FileChanged,
        HookPoint::TeammateIdle,
    ]
}
