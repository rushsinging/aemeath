//! 反思 phase：BeginReflection 转移 → Reflection activity → 端口执行 → 终态收口 →
//! ReflectionCompleted 返回进入前状态。反思任何 outcome 都不终止宿主 Run（与现状一致，
//! 宿主 Run 的取消由 `handle_interrupt`/`handle_step_control` 既有路径负责）。
//!
//! Interval / PreCompact / Manual 三触发共用本 phase（当前仅 Interval 接线，其余随
//! 后续批次接入）；调用点负责触发判定与材料收集。
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
