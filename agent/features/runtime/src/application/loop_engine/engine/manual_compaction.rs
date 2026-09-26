//! 手动压缩 Run 的执行阶段：runtime 受理 `/compact` 后，Run 由命令直接置为
//! `Compacting`；压缩完成回到 `DrainingInput`，收口仍由后续 drain 的
//! `EmptyAndSealed` 完成（`Completed` 的唯一来源）。
//!
//! 该阶段不产生 RunStep，也 **NEVER** 调用模型。

use super::*;

/// 手动压缩阶段的收口方向。
pub(super) enum ManualCompactionDirective {
    /// 压缩完成并已回到 `DrainingInput`，交给后续 drain 收口。
    Settled,
    /// 已取消或超时，Run 已进入终态。
    Terminal,
}

pub(super) async fn execute_manual_compaction(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
) -> Result<ManualCompactionDirective, LoopEngineError> {
    // 命令驱动置状态：Run 直接进入 `Compacting`，不借用 drain 结果推进。
    run.begin_manual_compaction()?;
    emit_events(run, execution, port).await?;
    let activity_id = match port.start_manual_compaction_activity() {
        Ok(activity_id) => Some(activity_id),
        Err(error) => {
            log::warn!(
                target: crate::LOG_TARGET,
                "[run_loop] 无法发布手动压缩 activity，继续执行压缩: {error}"
            );
            None
        }
    };
    let progress = match activity_id.as_ref() {
        Some(activity_id) => port.compact_progress_view(activity_id.clone()),
        None => std::sync::Arc::new(|_: sdk::CompactStageView, _: sdk::CompactWorkView| {}),
    };
    let Some(manual_compaction) = port.manual_compaction_mut() else {
        return Err(LoopEngineError::Adapter(
            "手动压缩 Run 未绑动手动压缩端口".to_string(),
        ));
    };
    let outcome = run_manual_compaction_phase(run, cancel, manual_compaction, progress).await?;
    match outcome {
        ManualCompactionPhaseOutcome::Ready(outcome) => {
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] 手动压缩完成 run_id={} outcome={outcome:?}",
                run.id(),
            );
            if let Some(activity_id) = activity_id {
                if let Err(error) = port.update_compaction_activity(
                    activity_id.clone(),
                    sdk::CompactStageView::Finalizing,
                ) {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[run_loop] 无法更新手动压缩 activity，继续收口: {error}"
                    );
                }
                if let Err(error) = port.finish_activity(activity_id, ActivityTerminal::Succeeded) {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[run_loop] 无法结束手动压缩 activity，继续收口: {error}"
                    );
                }
            }
            transition_and_emit(run, execution, port, RunTransition::CompactionCompleted).await?;
            Ok(ManualCompactionDirective::Settled)
        }
        ManualCompactionPhaseOutcome::Cancelled => {
            if let Some(activity_id) = activity_id {
                let _ = port.finish_activity(activity_id, ActivityTerminal::Cancelled);
            }
            terminate_interrupted_run(run, execution, port).await?;
            Ok(ManualCompactionDirective::Terminal)
        }
        ManualCompactionPhaseOutcome::TimedOut => {
            if let Some(activity_id) = activity_id {
                let _ = port.finish_activity(activity_id, ActivityTerminal::Terminated);
            }
            timeout_run(run, execution, port).await?;
            Ok(ManualCompactionDirective::Terminal)
        }
    }
}
