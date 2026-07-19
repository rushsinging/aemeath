//! Dispatcher 行为测试（#924 TDD 红灯）。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §4 §6 §10 与
//! `01-run-loop-integration.md` §1 §2。
//!
//! 本文件仅新增，不改 runtime / shared config。为避免依赖真实难制造的
//! Wait/IO 故障，使用私有 `Executor` port + `Scripted` fake 回放；
//! 生产 `ProcessDriver` 适配 `Executor` 由后续提交承接。
//!
//! **当前阶段（红灯）**：`Dispatcher::dispatch` 为空实现（返回 `proceed()`），
//! 永不调用 executor、不排序、不短路、不重试、不合成 StopFailure。
//! 因此以下测试应**全部失败**。后续提交在测试驱动下逐个转绿。

#![cfg(test)]

use tokio_util::sync::CancellationToken;

use crate::domain::invocation::{
    HookInvocation, HookPoint, PreToolUseInput, StopInput, UserPromptInput,
};
use crate::domain::outcome::{HookDirective, HookExecutionStatus, HookReason};
use crate::domain::subscription::{HookFailurePolicy, HookMatcher, HookSubscription};
use crate::ports::HookPort;

use super::{Dispatcher, ExecutionFault, ScriptStep, Scripted};

// ════════════════════════════════════════════════════════════
// 测试辅助
// ════════════════════════════════════════════════════════════

fn pre_tool_use(tool_name: &str) -> HookInvocation {
    HookInvocation::PreToolUse(PreToolUseInput {
        tool_name: tool_name.to_string(),
        tool_input: serde_json::json!({}),
    })
}

fn stop(turns: usize) -> HookInvocation {
    HookInvocation::Stop(StopInput { turns })
}

fn sub(point: HookPoint, command: &str) -> HookSubscription {
    HookSubscription::new(point, command)
}

// 各测试直接内联构造 Dispatcher + Scripted，以保持调用顺序与步骤入队的可读性。

// ════════════════════════════════════════════════════════════
// 1. matcher 过滤
// ════════════════════════════════════════════════════════════

/// `ToolName("Bash")` 仅在 PreToolUse(Bash) 时执行；`ToolName("Edit")` 不执行。
#[tokio::test]
async fn matcher_tool_name_only_executes_matching_subscription() {
    let subs = vec![
        sub(HookPoint::PreToolUse, "bash-cmd").with_matcher(HookMatcher::ToolName("Bash".into())),
        sub(HookPoint::PreToolUse, "edit-cmd").with_matcher(HookMatcher::ToolName("Edit".into())),
    ];
    let scripted = Scripted::from_steps([ScriptStep::ok_exit(0, "")]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());
    // 仅 bash-cmd 应执行，故只入队 1 步；edit-cmd 入队步会触发 exhausted panic。
    let outcome = dispatcher
        .dispatch(pre_tool_use("Bash"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.call_count(),
        1,
        "仅匹配的 Bash subscription 应执行"
    );
    assert_eq!(
        scripted.commands(),
        vec!["bash-cmd".to_string()],
        "未匹配的 Edit subscription 不应执行"
    );
    let _ = outcome;
}

/// `All` matcher 匹配任意 invocation（含不同工具名）。
#[tokio::test]
async fn matcher_all_matches_any_invocation() {
    let subs = vec![sub(HookPoint::PreToolUse, "all-cmd")];
    let scripted = Scripted::from_steps([ScriptStep::ok_exit(0, "")]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    dispatcher
        .dispatch(pre_tool_use("Anything"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 1, "All matcher 应匹配任意工具名");
}

/// 非工具 point（如 Stop）的 `ToolName` matcher 不命中；
/// 同 point 的 `All` matcher 仍命中并执行（搭配正向断言，避免空实现假绿）。
#[tokio::test]
async fn matcher_tool_name_does_not_match_non_tool_point() {
    let subs = vec![
        sub(HookPoint::Stop, "toolname-stop").with_matcher(HookMatcher::ToolName("Bash".into())),
        sub(HookPoint::Stop, "all-stop"),
    ];
    let scripted = Scripted::from_steps([ScriptStep::ok_exit(0, "")]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    dispatcher
        .dispatch(stop(1), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 1, "All matcher 应在 Stop 上执行");
    assert_eq!(
        scripted.commands(),
        vec!["all-stop".to_string()],
        "Stop 无工具名，ToolName matcher 不应命中；仅 All 命中"
    );
}

// ════════════════════════════════════════════════════════════
// 2. order + 声明顺序
// ════════════════════════════════════════════════════════════

/// 相同 order 时按声明顺序执行。
#[tokio::test]
async fn order_same_uses_declaration_order() {
    let subs = vec![
        sub(HookPoint::PreToolUse, "a"),
        sub(HookPoint::PreToolUse, "b"),
    ];
    let scripted = Scripted::from_steps([ScriptStep::ok_exit(0, ""), ScriptStep::ok_exit(0, "")]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.commands(),
        vec!["a".to_string(), "b".to_string()],
        "相同 order 应按声明顺序执行"
    );
}

/// 不同 order 时按 order 升序执行（order 小者先），与声明顺序相反。
#[tokio::test]
async fn order_ascending_before_declaration() {
    let subs = vec![
        sub(HookPoint::PreToolUse, "a").with_order(10),
        sub(HookPoint::PreToolUse, "b").with_order(0),
    ];
    let scripted = Scripted::from_steps([ScriptStep::ok_exit(0, ""), ScriptStep::ok_exit(0, "")]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.commands(),
        vec!["b".to_string(), "a".to_string()],
        "order=0 应先于 order=10 执行，覆盖声明顺序"
    );
}

// ════════════════════════════════════════════════════════════
// 3. Block 短路
// ════════════════════════════════════════════════════════════

/// 任一 subscription 返回 Block → 立即停止后续 subscription，整体 directive=Block。
#[tokio::test]
async fn block_short_circuits_remaining_subscriptions() {
    let subs = vec![
        sub(HookPoint::PreToolUse, "first"),
        sub(HookPoint::PreToolUse, "second"),
    ];
    // first 返回非零 exit（主动 Block）；second 不应执行，故不为其入队步。
    let scripted = Scripted::from_steps([ScriptStep::ok_exit(2, "denied")]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.call_count(),
        1,
        "Block 后剩余 subscription 不应执行"
    );
    assert!(
        matches!(
            outcome.directive,
            HookDirective::Block {
                reason: HookReason::ExitCode { code: 2, .. }
            }
        ),
        "首个非零 exit 应短路为 Block{{ExitCode}}，实际 = {:?}",
        outcome.directive
    );
}

// ════════════════════════════════════════════════════════════
// 4. Context 顺序合并
// ════════════════════════════════════════════════════════════

/// 多个 ContinueWithContext 的 context 按执行顺序拼接。
#[tokio::test]
async fn context_merged_in_execution_order() {
    let subs = vec![
        sub(HookPoint::PreToolUse, "a"),
        sub(HookPoint::PreToolUse, "b"),
    ];
    let scripted = Scripted::from_steps([
        ScriptStep::ok_json(r#"{"additionalContext":"ctx-a"}"#),
        ScriptStep::ok_json(r#"{"additionalContext":"ctx-b"}"#),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 2);
    match outcome.directive {
        HookDirective::ContinueWithContext { context } => {
            assert_eq!(context, "ctx-a\nctx-b", "context 应按顺序以换行合并 a→b");
        }
        other => panic!("两次 ContinueWithContext 应合并为 ContinueWithContext，实际 = {other:?}"),
    }
}

// ════════════════════════════════════════════════════════════
// 5. UpdatedInput 串联（结构位置）
// ════════════════════════════════════════════════════════════

/// subscription A 的 UpdatedInput 必须真正替换 PreToolUse 的 `tool_input`，
/// 落在枚举 payload 的结构位置（`PreToolUse.tool_input`），
/// **而不是**仅往 enum JSON 顶层插键。
#[tokio::test]
async fn updated_input_replaces_tool_input_at_payload_location() {
    let subs = vec![
        sub(HookPoint::PreToolUse, "a"),
        sub(HookPoint::PreToolUse, "b"),
    ];
    let scripted = Scripted::from_steps([
        ScriptStep::ok_json(r#"{"hookSpecificOutput":{"updatedInput":{"rewritten":true}}}"#),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("Bash"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 2, "A、B 均应执行");
    let b_stdin = &scripted.calls()[1].stdin;
    // UpdatedInput 必须落到 PreToolUse.tool_input，而非 enum 顶层或 hookSpecificOutput。
    assert_eq!(
        b_stdin["PreToolUse"]["tool_input"],
        serde_json::json!({"rewritten": true}),
        "B 的 stdin 应在 PreToolUse.tool_input 携带被改写的值，实际 = {b_stdin}"
    );
    assert!(
        b_stdin.get("rewritten").is_none(),
        "UpdatedInput 不得泄漏到 enum JSON 顶层，实际 = {b_stdin}"
    );
    assert!(
        b_stdin["PreToolUse"].get("hookSpecificOutput").is_none(),
        "UpdatedInput 不得原样保留为 hookSpecificOutput，实际 = {b_stdin}"
    );
    assert!(
        b_stdin["PreToolUse"]["tool_input"]
            .get("original")
            .is_none(),
        "原 tool_input 应被整体替换而非浅合并，实际 = {b_stdin}"
    );
    // 最终 directive 仍携带最后一次 UpdatedInput 的值（供调用方重新校验）。
    match outcome.directive {
        HookDirective::ContinueWithUpdatedInput { input } => {
            assert_eq!(input, serde_json::json!({"rewritten": true}));
        }
        other => panic!("应为 ContinueWithUpdatedInput，实际 = {other:?}"),
    }
}

/// `can_modify_input` 的 String 字段（UserPromptSubmit.prompt）也必须被整体替换，
/// 且 updatedInput 取 JSON 字符串形态落到 payload 结构位置。
#[tokio::test]
async fn updated_input_replaces_user_prompt_at_payload_location() {
    let subs = vec![
        sub(HookPoint::UserPromptSubmit, "a"),
        sub(HookPoint::UserPromptSubmit, "b"),
    ];
    let scripted = Scripted::from_steps([
        ScriptStep::ok_json(r#"{"hookSpecificOutput":{"updatedInput":"rewritten-prompt"}}"#),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    dispatcher
        .dispatch(
            HookInvocation::UserPromptSubmit(UserPromptInput {
                prompt: "original".to_string(),
            }),
            &CancellationToken::new(),
        )
        .await;

    let b_stdin = &scripted.calls()[1].stdin;
    assert_eq!(
        b_stdin["UserPromptSubmit"]["prompt"],
        serde_json::json!("rewritten-prompt"),
        "UserPromptSubmit.prompt 应被 updatedInput 整体替换，实际 = {b_stdin}"
    );
    assert!(
        b_stdin.get("rewritten-prompt").is_none(),
        "updatedInput 不得泄漏到 enum 顶层，实际 = {b_stdin}"
    );
}

// ════════════════════════════════════════════════════════════
// 6. ExecutionFailed 各类最多重试 3 次
// ════════════════════════════════════════════════════════════

/// 参数化：每种协议级故障连续发生时，最多重试 MAX_ATTEMPTS(3) 次。
async fn assert_fault_retries_three_times(kind: ExecutionFault) {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([
        ScriptStep::fault(kind),
        ScriptStep::fault(kind),
        ScriptStep::fault(kind),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.call_count(),
        3,
        "{kind:?} 应重试至多 3 次（含首次）"
    );
    assert!(
        outcome
            .executions
            .iter()
            .all(|e| matches!(e.status, HookExecutionStatus::ExecutionFailed { .. })),
        "{kind:?} 重试耗尽后所有 execution 应为 ExecutionFailed"
    );
}

#[tokio::test]
async fn retry_spawn_up_to_three() {
    assert_fault_retries_three_times(ExecutionFault::Spawn).await;
}

#[tokio::test]
async fn retry_wait_up_to_three() {
    assert_fault_retries_three_times(ExecutionFault::Wait).await;
}

#[tokio::test]
async fn retry_io_up_to_three() {
    assert_fault_retries_three_times(ExecutionFault::Io).await;
}

#[tokio::test]
async fn retry_timeout_up_to_three() {
    assert_fault_retries_three_times(ExecutionFault::Timeout).await;
}

#[tokio::test]
async fn retry_invalid_json_up_to_three() {
    // exit 0 + 非法 JSON → classify InvalidJson → ExecutionFailed 重试。
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([
        ScriptStep::ok_json("not json"),
        ScriptStep::ok_json("not json"),
        ScriptStep::ok_json("not json"),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 3, "InvalidJson 应重试至多 3 次");
    assert!(
        outcome
            .executions
            .iter()
            .all(|e| matches!(e.status, HookExecutionStatus::ExecutionFailed { .. })),
        "InvalidJson 重试耗尽后所有 execution 应为 ExecutionFailed"
    );
}

// ════════════════════════════════════════════════════════════
// 7. 第三次成功
// ════════════════════════════════════════════════════════════

/// 两次 ExecutionFailed 后第三次成功 → 不再重试，directive 为成功结果。
#[tokio::test]
async fn third_attempt_succeeds_after_two_failures() {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 3, "第三次成功应恰好尝试 3 次");
    assert!(
        matches!(outcome.directive, HookDirective::Continue),
        "第三次成功（exit 0 + 空输出）应为 Continue，实际 = {:?}",
        outcome.directive
    );
}

/// 两次 ExecutionFailed 后第三次成功 → HookOutcome.executions 必须保留全部
/// 三条 attempt 明细（attempts 1/2/3），前两条 ExecutionFailed、第三条 Success。
///
/// 回归测试：此前 `AttemptOutcome::Success` 仅携带最终成功的 execution，丢弃了
/// prior executions（此前失败的 attempt），导致 HookOutcome.executions 丢失重试轨迹。
#[tokio::test]
async fn third_success_preserves_all_three_attempt_details() {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        outcome.executions.len(),
        3,
        "两次失败 + 第三次成功应保留 3 条 execution 明细，实际 = {:?}",
        outcome.executions
    );
    // 前两条：ExecutionFailed，attempts 递增 1→2。
    assert!(
        matches!(
            outcome.executions[0].status,
            HookExecutionStatus::ExecutionFailed { .. }
        ),
        "第 1 条 attempt 应为 ExecutionFailed，实际 = {:?}",
        outcome.executions[0].status
    );
    assert_eq!(outcome.executions[0].attempts, 1, "第 1 条 attempts 应为 1");
    assert!(
        matches!(
            outcome.executions[1].status,
            HookExecutionStatus::ExecutionFailed { .. }
        ),
        "第 2 条 attempt 应为 ExecutionFailed，实际 = {:?}",
        outcome.executions[1].status
    );
    assert_eq!(outcome.executions[1].attempts, 2, "第 2 条 attempts 应为 2");
    // 第三条：Success，attempts 为 3。
    assert!(
        matches!(outcome.executions[2].status, HookExecutionStatus::Success),
        "第 3 条 attempt 应为 Success，实际 = {:?}",
        outcome.executions[2].status
    );
    assert_eq!(outcome.executions[2].attempts, 3, "第 3 条 attempts 应为 3");
}

// ════════════════════════════════════════════════════════════
// 8. exit 1/2/127 一次（业务 Block 不重试）
// ════════════════════════════════════════════════════════════

#[tokio::test]
async fn exit_code_1_blocks_once_without_retry() {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([ScriptStep::ok_exit(1, "nope")]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 1, "业务 Block（exit 1）不应重试");
    assert!(matches!(
        outcome.directive,
        HookDirective::Block {
            reason: HookReason::ExitCode { code: 1, .. }
        }
    ));
}

#[tokio::test]
async fn exit_code_2_blocks_once_without_retry() {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([ScriptStep::ok_exit(2, "")]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 1, "exit 2 是业务 Block，不重试");
    assert!(matches!(
        outcome.directive,
        HookDirective::Block {
            reason: HookReason::ExitCode { code: 2, .. }
        }
    ));
}

#[tokio::test]
async fn exit_code_127_blocks_once_without_retry() {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([ScriptStep::ok_exit(127, "command not found")]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 1, "exit 127 是业务 Block，不重试");
    assert!(matches!(
        outcome.directive,
        HookDirective::Block {
            reason: HookReason::ExitCode { code: 127, .. }
        }
    ));
}

// ════════════════════════════════════════════════════════════
// 9. Cancellation 不重试
// ════════════════════════════════════════════════════════════

/// 执行返回 Cancelled → 立即终止，不重试。
#[tokio::test]
async fn cancellation_is_not_retried() {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([ScriptStep::fault(ExecutionFault::Cancelled)]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.call_count(),
        1,
        "Cancelled 不应触发重试（仅执行 1 次）"
    );
    let _ = outcome;
}

// ════════════════════════════════════════════════════════════
// 10. 普通默认 Continue
// ════════════════════════════════════════════════════════════

/// 普通 Hook（failure_policy=None）ExecutionFailed 重试耗尽 → 默认 Continue。
#[tokio::test]
async fn normal_hook_default_continue_on_exhausted_failures() {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 3);
    assert!(
        matches!(outcome.directive, HookDirective::Continue),
        "未配置 failure_policy 的普通 Hook 重试耗尽应默认 Continue，实际 = {:?}",
        outcome.directive
    );
    assert!(
        !outcome.executions.is_empty(),
        "重试耗尽应保留 ExecutionFailed 明细"
    );
}

// ════════════════════════════════════════════════════════════
// 11. 配置 Block
// ════════════════════════════════════════════════════════════

/// 普通 Hook 配置 failure_policy=Block → ExecutionFailed 重试耗尽后 Block。
#[tokio::test]
async fn configured_block_policy_blocks_on_exhausted_failures() {
    let subs =
        vec![sub(HookPoint::PreToolUse, "cmd").with_failure_policy(HookFailurePolicy::Block)];
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(scripted.call_count(), 3);
    assert!(
        matches!(outcome.directive, HookDirective::Block { .. }),
        "配置 failure_policy=Block 的 Hook 重试耗尽应 Block，实际 = {:?}",
        outcome.directive
    );
}

// ════════════════════════════════════════════════════════════
// 12. Stop 耗尽 → Block(StopHookExecutionFailed) + 一次 StopFailure
// ════════════════════════════════════════════════════════════

/// Stop subscription 3 次执行失败 → 合成 Block(StopHookExecutionFailed)，
/// 并尽力派发**恰好一次** StopFailure 通知。
#[tokio::test]
async fn stop_exhausted_synthesizes_block_and_one_stop_failure() {
    let subs = vec![
        sub(HookPoint::Stop, "stop-cmd"),
        sub(HookPoint::StopFailure, "stopfail-cmd"),
    ];
    // 3 次 Stop 失败 + 1 次 StopFailure 成功。
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(stop(1), &CancellationToken::new())
        .await;

    let commands = scripted.commands();
    assert_eq!(
        scripted.call_count(),
        4,
        "应执行 3 次 Stop + 1 次 StopFailure，实际调用 = {commands:?}"
    );
    assert_eq!(
        commands.iter().filter(|c| c == &"stopfail-cmd").count(),
        1,
        "StopFailure 应恰好派发一次，实际 commands = {commands:?}"
    );
    assert!(
        matches!(
            outcome.directive,
            HookDirective::Block {
                reason: HookReason::StopHookExecutionFailed { .. }
            }
        ),
        "Stop 重试耗尽应合成 Block(StopHookExecutionFailed)，实际 = {:?}",
        outcome.directive
    );
}

// ════════════════════════════════════════════════════════════
// 13. StopFailure 自身不递归
// ════════════════════════════════════════════════════════════

/// StopFailure subscription 自身执行失败时：
/// - 不再合成新的 StopFailure（不递归）；
/// - 不为 StopFailure 合成 Block（它只是观察点）；
/// - 总执行次数有上界（3 Stop + 至多 3 StopFailure = 6，无第 7 次）。
#[tokio::test]
async fn stop_failure_does_not_recurse_on_own_failure() {
    let subs = vec![
        sub(HookPoint::Stop, "stop-cmd"),
        sub(HookPoint::StopFailure, "stopfail-cmd"),
    ];
    // Stop 3 次失败 + StopFailure 自身 3 次失败（不应再有第 4 次 StopFailure 会话）。
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(stop(1), &CancellationToken::new())
        .await;

    let commands = scripted.commands();
    assert!(
        scripted.call_count() <= 6,
        "StopFailure 失败不应递归派发新的 StopFailure，总调用应 ≤ 6，实际 = {commands:?}"
    );
    assert_eq!(
        commands.iter().filter(|c| c == &"stopfail-cmd").count(),
        3,
        "StopFailure 应只有一个会话（至多 3 次重试），无第二次合成，实际 = {commands:?}"
    );
    // StopFailure 失败不得改变已合成的 Stop Block 语义。
    assert!(
        matches!(
            outcome.directive,
            HookDirective::Block {
                reason: HookReason::StopHookExecutionFailed { .. }
            }
        ),
        "StopFailure 失败不得改写 Stop 的 Block 语义，实际 = {:?}",
        outcome.directive
    );
}

// ════════════════════════════════════════════════════════════
// 13b. StopFailure subscriptions 也遵循 enabled + matcher + order 稳定规则
// ════════════════════════════════════════════════════════════

/// StopFailure 派发必须复用主 dispatch 的 enabled / matcher / order 规则：
/// - disabled 的 StopFailure subscription 不执行；
/// - ToolName matcher 在无工具名的 StopFailure invocation 上不命中；
/// - 同 point 多 subscription 按 order + 声明顺序稳定执行。
#[tokio::test]
async fn stop_failure_respects_enabled_matcher_and_order() {
    let disabled = {
        let mut s = sub(HookPoint::StopFailure, "sf-disabled");
        s.enabled = false;
        s
    };
    let subs = vec![
        sub(HookPoint::Stop, "stop-cmd"),
        // ToolName matcher：StopFailure 无工具名，永不命中。
        sub(HookPoint::StopFailure, "sf-toolname")
            .with_matcher(HookMatcher::ToolName("Bash".into())),
        // order 10 先声明、order 0 后声明：稳定排序后 order=0 先执行。
        sub(HookPoint::StopFailure, "sf-late").with_order(10),
        sub(HookPoint::StopFailure, "sf-early").with_order(0),
        disabled,
    ];
    // 3 次 Stop 失败 + 2 次 StopFailure 成功（按 order 0→10）。
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::ok_exit(0, ""),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    dispatcher
        .dispatch(stop(1), &CancellationToken::new())
        .await;

    let all_cmds = scripted.commands();
    let sf_cmds: Vec<&String> = all_cmds.iter().filter(|c| c.starts_with("sf-")).collect();
    assert_eq!(
        sf_cmds,
        vec![&"sf-early".to_string(), &"sf-late".to_string()],
        "StopFailure 应按 order 升序执行，且 disabled / 不匹配 matcher 的 subscription 不执行，实际 = {all_cmds:?}"
    );
}

/// StopFailure subscription 的执行明细必须并入原 Stop HookOutcome.executions，
/// 而非被丢弃。
#[tokio::test]
async fn stop_failure_executions_merge_into_stop_outcome() {
    let subs = vec![
        sub(HookPoint::Stop, "stop-cmd"),
        sub(HookPoint::StopFailure, "sf-cmd"),
    ];
    // 3 次 Stop 失败（空 stdout）+ 1 次 StopFailure 成功，stdout 带 marker（合法 JSON → Continue）。
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::ok_json(r#"{"sf":"MARKER"}"#),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(stop(1), &CancellationToken::new())
        .await;

    assert!(
        outcome
            .executions
            .iter()
            .any(|e| e.stdout.contains("MARKER")),
        "StopFailure 执行明细应并入 Stop outcome.executions，实际 executions = {:?}",
        outcome.executions
    );
}

// ════════════════════════════════════════════════════════════
// 13c. StopFailure 多 subscription：某个默认失败耗尽后继续后续观察 subscription（best effort）
// ════════════════════════════════════════════════════════════

/// StopFailure 有多个 subscription 时，第一个（默认 policy）失败耗尽后，
/// 必须继续执行后续观察 subscription（best effort），不得因前者耗尽而提前终止。
#[tokio::test]
async fn stop_failure_continues_to_next_observer_after_one_exhausts() {
    let subs = vec![
        sub(HookPoint::Stop, "stop-cmd"),
        sub(HookPoint::StopFailure, "sf-a"),
        sub(HookPoint::StopFailure, "sf-b"),
    ];
    // 3 次 Stop 失败 + sf-a 3 次失败耗尽 + sf-b 成功。
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(stop(1), &CancellationToken::new())
        .await;

    let commands = scripted.commands();
    assert_eq!(
        scripted.call_count(),
        7,
        "sf-a 耗尽后应继续 sf-b，实际调用 = {commands:?}"
    );
    assert!(
        commands.contains(&"sf-b".to_string()),
        "sf-b 应被执行（best effort），实际 = {commands:?}"
    );
    // sf-b 成功明细应并入 outcome.executions。
    assert!(
        outcome
            .executions
            .iter()
            .any(|e| matches!(e.status, HookExecutionStatus::Success)),
        "sf-b 成功明细应并入 executions，实际 = {:?}",
        outcome.executions
    );
    // 观察结果不改写已合成的 Stop Block 语义。
    assert!(
        matches!(
            outcome.directive,
            HookDirective::Block {
                reason: HookReason::StopHookExecutionFailed { .. }
            }
        ),
        "StopFailure 观察结果不得改写 Stop Block 语义，实际 = {:?}",
        outcome.directive
    );
}

// ════════════════════════════════════════════════════════════
// 13d. exit_code=None → MissingExitCode → ExecutionFailed 重试至多 3 次
// ════════════════════════════════════════════════════════════

/// `RawExecution.exit_code=None`（进程未正常退出）必须分类为 `MissingExitCode`，
/// 进入 ExecutionFailed 可重试路径，最多重试 3 次；**不得**按空 stdout 误判为 Continue。
#[tokio::test]
async fn missing_exit_code_retries_up_to_three() {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([
        ScriptStep::no_exit_code(),
        ScriptStep::no_exit_code(),
        ScriptStep::no_exit_code(),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.call_count(),
        3,
        "exit_code=None 应重试至多 3 次（含首次）"
    );
    assert!(
        outcome
            .executions
            .iter()
            .all(|e| matches!(e.status, HookExecutionStatus::ExecutionFailed { .. })),
        "exit_code=None 重试耗尽后所有 execution 应为 ExecutionFailed"
    );
}

// ════════════════════════════════════════════════════════════
// 13e. 普通 subscription 默认 policy 耗尽后继续后续 subscription 并聚合成功 directive
// ════════════════════════════════════════════════════════════

/// 普通 subscription 默认（None）policy ExecutionFailed 三次耗尽时，
/// 必须继续执行后续 subscription，保留前三次失败明细并聚合后续成功 directive。
///
/// 回归测试：此前 Exhausted 分支无条件 return，错误地在默认 policy 下短路，
/// 既丢失了后续成功 directive 聚合，也跳过了后续 subscription。
#[tokio::test]
async fn default_policy_exhausted_continues_to_next_subscription() {
    let subs = vec![
        sub(HookPoint::PreToolUse, "a"),
        sub(HookPoint::PreToolUse, "b"),
    ];
    // a 三次失败耗尽；b 成功返回 ContinueWithContext。
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::ok_json(r#"{"additionalContext":"ctx-b"}"#),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.call_count(),
        4,
        "a 耗尽后必须继续执行 b，实际调用 = {:?}",
        scripted.commands()
    );
    // a 的三次失败明细必须保留。
    let failed_count = outcome
        .executions
        .iter()
        .filter(|e| matches!(e.status, HookExecutionStatus::ExecutionFailed { .. }))
        .count();
    assert_eq!(
        failed_count, 3,
        "a 的三次 ExecutionFailed 明细必须保留，实际 executions = {:?}",
        outcome.executions
    );
    // 后续 b 成功的 directive（ContinueWithContext）必须聚合。
    assert!(
        matches!(
            outcome.directive,
            HookDirective::ContinueWithContext { ref context } if context == "ctx-b"
        ),
        "默认 policy 耗尽后应继续聚合后续成功 directive，实际 = {:?}",
        outcome.directive
    );
}

/// 显式 `failure_policy=Continue` 的普通 Hook 耗尽后也应继续后续 subscription。
#[tokio::test]
async fn continue_policy_exhausted_continues_to_next_subscription() {
    let subs = vec![
        sub(HookPoint::PreToolUse, "a").with_failure_policy(HookFailurePolicy::Continue),
        sub(HookPoint::PreToolUse, "b"),
    ];
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::ok_exit(0, ""),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.call_count(),
        4,
        "Continue policy 耗尽后应继续 b，实际 = {:?}",
        scripted.commands()
    );
    assert!(
        matches!(outcome.directive, HookDirective::Continue),
        "最终应聚合为 Continue，实际 = {:?}",
        outcome.directive
    );
}

// ════════════════════════════════════════════════════════════
// 13f. Block policy 耗尽短路：后续 subscription 不执行
// ════════════════════════════════════════════════════════════

/// 配置 `failure_policy=Block` 的前置闸门 ExecutionFailed 重试耗尽后
/// 合成 Block(PolicyBlock) 并短路，后续 subscription 不执行。
#[tokio::test]
async fn block_policy_exhausted_short_circuits_remaining_subscriptions() {
    let subs = vec![
        sub(HookPoint::PreToolUse, "a").with_failure_policy(HookFailurePolicy::Block),
        sub(HookPoint::PreToolUse, "b"),
    ];
    // a 三次失败耗尽 → Block 短路；b 不应执行，不为其入队步。
    let scripted = Scripted::from_steps([
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
        ScriptStep::fault(ExecutionFault::Io),
    ]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.call_count(),
        3,
        "Block policy 耗尽应短路，b 不应执行，实际 = {:?}",
        scripted.commands()
    );
    assert!(
        matches!(
            outcome.directive,
            HookDirective::Block {
                reason: HookReason::PolicyBlock { .. }
            }
        ),
        "Block policy 耗尽应合成 Block{{PolicyBlock}}，实际 = {:?}",
        outcome.directive
    );
}

// ════════════════════════════════════════════════════════════
// 14. Cancelled 也是一次 attempt：保留 ExecutionFailed / 取消明细
// ════════════════════════════════════════════════════════════

/// 执行返回 Cancelled 视作一次 attempt，必须在 executions 中保留一条
/// ExecutionFailed 明细（错误文本为中文），而非静默丢弃。
#[tokio::test]
async fn cancelled_records_execution_failed_with_chinese_detail() {
    let subs = vec![sub(HookPoint::PreToolUse, "cmd")];
    let scripted = Scripted::from_steps([ScriptStep::fault(ExecutionFault::Cancelled)]);
    let dispatcher = Dispatcher::with_scripted(subs, scripted.clone());

    let outcome = dispatcher
        .dispatch(pre_tool_use("X"), &CancellationToken::new())
        .await;

    assert_eq!(
        scripted.call_count(),
        1,
        "Cancelled 不应触发重试（仅执行 1 次）"
    );
    assert_eq!(
        outcome.executions.len(),
        1,
        "Cancelled 这一次 attempt 的明细必须保留，实际 = {:?}",
        outcome.executions
    );
    match &outcome.executions[0].status {
        HookExecutionStatus::ExecutionFailed { error } => {
            assert!(
                error.contains('取') && error.contains('消'),
                "Cancelled 的 ExecutionFailed 错误文本应为非空中文（含「取消」），实际 = {error:?}"
            );
        }
        other => panic!("Cancelled 这一次 attempt 应记为 ExecutionFailed，实际 status = {other:?}"),
    }
}
