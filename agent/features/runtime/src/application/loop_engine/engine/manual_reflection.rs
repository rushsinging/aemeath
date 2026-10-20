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

/// 手动反思阶段的收口方向。统一异步链路（#2002）：派发即收口回到
/// `DrainingInput`，取消/超时由 detached 内部处理（Run 侧 Terminal 方向已退役）。
pub(super) enum ManualReflectionDirective {
    /// 派发已收口并回到 `DrainingInput`，交给后续 drain 收口。
    Settled,
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
        // 统一异步链路（#2002）：Manual 派发即收口——端口层已把 Detached（派发
        // 成功）/BusySkipped/DisabledSkipped 折叠为 Ready(Succeeded/Failed)，
        // Run 立即回 DrainingInput，NEVER 等待执行终态；取消/超时由 detached
        // 任务内部处理（cancel 不影响异步，D2），Run 侧 Cancelled/TimedOut
        // 分支已退役。
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
    }
}
