//! 手动反思 Run 的执行阶段：runtime 受理 `/reflect-now` 后，Run 由命令直接置为
//! `Reflecting`；反思完成回到 `DrainingInput`，收口仍由后续 drain 的
//! `EmptyAndSealed` 完成（`Completed` 的唯一来源）。
//!
//! 该阶段不产生 RunStep，也 **NEVER** 调用模型、不走 `ContextPort::build_window`。
//!
//! 失败路径与 `manual_compaction.rs` / engine reflection phase 对齐：activity
//! 发布失败只记 warn 继续执行；端口缺失或端口 Err 先把 Reflection activity 按
//! Failed 收口、状态机经 `ReflectionCompleted` 回到 `DrainingInput` 后才上抛错误
//! ——NEVER 留下 Running 态的 activity 或悬挂的 `Reflecting`。

use super::*;
use crate::application::reflection::ReflectionTaskCompletionStatus;

/// 手动反思阶段的收口方向。
pub(super) enum ManualReflectionDirective {
    /// 反思已完成并回到 `DrainingInput`，交给后续 drain 收口。
    Settled,
    /// 已取消或超时，Run 已进入终态。
    Terminal,
}

pub(super) async fn execute_manual_reflection(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
) -> Result<ManualReflectionDirective, LoopEngineError> {
    // 命令驱动置状态：Run 直接进入 `Reflecting`，不借用 drain 结果推进。
    run.begin_manual_reflection()?;
    emit_events(run, execution, port).await?;
    let activity_id = match port.start_manual_reflection_activity() {
        Ok(activity_id) => Some(activity_id),
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "[run_loop] 无法发布手动反思 activity，继续执行反思: {error}"
            );
            None
        }
    };
    let outcome = match port.manual_reflection_mut() {
        Some(manual_reflection) => {
            manual_reflection
                .run_manual_reflection(run.id(), cancel)
                .await
        }
        None => Err(LoopEngineError::Adapter(
            "手动反思 Run 未绑手动反思端口".to_string(),
        )),
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            // 端口缺失/端口 Err 是契约违约（端口应把失败折叠进 outcome）：先把观测
            // 与状态机收干净，再上抛错误，NEVER 留下 Running 态的 Reflection
            // activity 或悬挂的 `Reflecting`。
            if let Some(activity_id) = activity_id {
                let _ = port.finish_activity(activity_id, ActivityTerminal::Failed);
            }
            transition_and_emit(run, execution, port, RunTransition::ReflectionCompleted).await?;
            return Err(error);
        }
    };
    match outcome {
        ManualReflectionOutcome::Ready(status) => {
            let terminal = if status == ReflectionTaskCompletionStatus::Succeeded {
                ActivityTerminal::Succeeded
            } else {
                ActivityTerminal::Failed
            };
            if let Some(activity_id) = activity_id {
                if let Err(error) = port.finish_activity(activity_id, terminal) {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[run_loop] 无法结束手动反思 activity，继续收口: {error}"
                    );
                }
            }
            transition_and_emit(run, execution, port, RunTransition::ReflectionCompleted).await?;
            Ok(ManualReflectionDirective::Settled)
        }
        ManualReflectionOutcome::Cancelled => {
            if let Some(activity_id) = activity_id {
                let _ = port.finish_activity(activity_id, ActivityTerminal::Cancelled);
            }
            // 用户取消（Esc/Ctrl-C 经 `cancel_current_run` cancel root token）→
            // UserExit 终止语义，与 registry 侧 control 一致；NEVER 伪装成
            // SessionShutdown（会话关闭）。
            terminate_interrupted_run(run, execution, port, sdk::RunTerminationReason::UserExit)
                .await?;
            Ok(ManualReflectionDirective::Terminal)
        }
        ManualReflectionOutcome::TimedOut => {
            if let Some(activity_id) = activity_id {
                let _ = port.finish_activity(activity_id, ActivityTerminal::Terminated);
            }
            timeout_run(run, execution, port).await?;
            Ok(ManualReflectionDirective::Terminal)
        }
    }
}
