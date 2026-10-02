//! 反思 phase：BeginReflection 转移 → Reflection activity → 端口执行 → 终态收口 →
//! ReflectionCompleted 返回进入前状态。反思任何 outcome 都不终止宿主 Run（与现状一致，
//! 宿主 Run 的取消由 `handle_interrupt`/`handle_step_control` 既有路径负责）。
//!
//! Interval / PreCompact 走本文件的 shared phase（`run_reflection_phase`），Manual
//! 走 `engine/manual_reflection.rs` 的独立入口；三者仅共享底层
//! `chat::reflection::run` / 端口层编排，状态机与 activity 由各自 engine/port 负责。
//! 调用点负责触发判定与材料收集。
//!
//! 观测降级的有意设计：activity 发布/收口失败只记 warn 并继续执行（反思是
//! best-effort 观测，NEVER 因观测失败阻断反思本体）；端口 Err 视为契约违约，
//! activity 按 Failed 收口、状态机照常返回后错误才上抛。

use super::*;
use crate::application::reflection::{ReflectionRunOutcome, ReflectionTaskCompletionStatus};

pub(super) async fn run_reflection_phase(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    trigger: crate::application::reflection::ReflectionTaskTrigger,
    messages: Vec<share::message::Message>,
    run_step_id: Option<&sdk::RunStepId>,
    coverage_end: Option<u64>,
    step_cancel: &CancellationToken,
) -> Result<(), LoopEngineError> {
    transition_and_emit(run, execution, port, RunTransition::BeginReflection).await?;
    let activity_id = match port.start_reflection_activity(trigger) {
        Ok(id) => Some(id),
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "[run_loop] 无法发布反思 activity，继续执行反思: {error}"
            );
            None
        }
    };
    let outcome = match port.reflection_mut() {
        Some(reflection) => {
            reflection
                .run_reflection(
                    trigger,
                    messages,
                    run.id(),
                    run_step_id,
                    coverage_end,
                    step_cancel.clone(),
                )
                .await
        }
        None => {
            // 端口未绑定视为跳过：状态机已经进入 Reflecting，必须照常收口返回。
            log::warn!(
                target: crate::LOG_TARGET,
                "[run_loop] 反思端口未绑定，跳过反思执行"
            );
            Ok(ReflectionRunOutcome::DisabledSkipped)
        }
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            // 端口 Err 是契约违约（端口应把失败折叠进 outcome）：先把观测与状态机
            // 收干净，再上抛错误，NEVER 留下 Running 态悬挂的 Reflection activity。
            if let Some(activity_id) = activity_id {
                let _ = port.finish_activity(activity_id, ActivityTerminal::Failed);
            }
            transition_and_emit(run, execution, port, RunTransition::ReflectionCompleted).await?;
            return Err(error);
        }
    };
    if let Some(activity_id) = activity_id {
        let terminal = match &outcome {
            ReflectionRunOutcome::Completed(completion) => match completion.status {
                ReflectionTaskCompletionStatus::Succeeded => ActivityTerminal::Succeeded,
                ReflectionTaskCompletionStatus::Failed => ActivityTerminal::Failed,
                ReflectionTaskCompletionStatus::Cancelled => ActivityTerminal::Cancelled,
                ReflectionTaskCompletionStatus::TimedOut => ActivityTerminal::Terminated,
            },
            ReflectionRunOutcome::DisabledSkipped => ActivityTerminal::Cancelled,
        };
        let _ = port.finish_activity(activity_id, terminal);
    }
    transition_and_emit(run, execution, port, RunTransition::ReflectionCompleted).await?;
    Ok(())
}

/// PreCompact 插入点：两处自动压缩（needs_compaction 与 ModelContextExceeded）在
/// `ContextCompactionOutcome::Ready`、压缩 activity 收口之后、`CompactionCompleted`
/// 转移之前调用。材料由 compaction observer 在 Committed 时暂存进反思端口的共享槽，
/// 此处取出后在 Compacting 内完成 `Compacting → Reflecting → Compacting` 往返，
/// 再放行压缩收口；未暂存（Skipped/未绑定反思端口）为 noop。
pub(super) async fn run_pre_compact_reflection_phase_if_staged(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    run_step_id: &sdk::RunStepId,
    step_cancel: &CancellationToken,
) -> Result<(), LoopEngineError> {
    let messages = port
        .reflection_mut()
        .and_then(|reflection| reflection.take_pre_compact_messages());
    let Some(messages) = messages else {
        return Ok(());
    };
    // PreCompact 反思的是被丢弃段（非 session 历史增量），不推进游标（None）。
    run_reflection_phase(
        run,
        execution,
        port,
        crate::application::reflection::ReflectionTaskTrigger::PreCompact,
        messages,
        Some(run_step_id),
        None,
        step_cancel,
    )
    .await
}
