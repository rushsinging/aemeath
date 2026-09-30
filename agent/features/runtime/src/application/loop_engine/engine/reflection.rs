//! 反思 phase：BeginReflection 转移 → Reflection activity → 端口执行 → 终态收口 →
//! ReflectionCompleted 返回进入前状态。反思任何 outcome 都不终止宿主 Run（与现状一致，
//! 宿主 Run 的取消由 `handle_interrupt`/`handle_step_control` 既有路径负责）。
//!
//! Interval / PreCompact / Manual 三触发共用本 phase；调用点负责触发判定与材料收集。

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
                .await?
        }
        None => {
            // 端口未绑定视为跳过：状态机已经进入 Reflecting，必须照常收口返回。
            log::warn!(
                target: crate::LOG_TARGET,
                "[run_loop] 反思端口未绑定，跳过反思执行"
            );
            ReflectionRunOutcome::DisabledSkipped
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
