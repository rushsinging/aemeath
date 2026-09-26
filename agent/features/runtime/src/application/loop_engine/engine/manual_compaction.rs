//! 手动压缩 Run 的执行阶段：在输入通道已 seal 且无输入时兑现
//! `RunIntent::ManualCompaction`。
//!
//! 该阶段不产生 RunStep，也 **NEVER** 调用模型：压缩完成后经
//! `CompactionOnlySettled` 回到 `DrainingInput`，由第二次 drain 的
//! `EmptyAndSealed` 收口 `Completed`。

use super::*;

pub(super) async fn execute_manual_compaction(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
) -> Result<(), LoopEngineError> {
    // 首轮 drain 已确认输入通道 seal 且无输入：把「压缩意图」作为内部
    // continuation 消费，先进入 PreparingContext，再开始压缩。
    run.apply_drain_decision(DrainDecision::InternalContinuation, None)?;
    emit_events(run, execution, port).await?;
    transition_and_emit(run, execution, port, RunTransition::BeginCompaction).await?;
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
            transition_and_emit(run, execution, port, RunTransition::CompactionOnlySettled).await?;
            Ok(())
        }
        ManualCompactionPhaseOutcome::Cancelled => {
            if let Some(activity_id) = activity_id {
                let _ = port.finish_activity(activity_id, ActivityTerminal::Cancelled);
            }
            terminate_interrupted_run(run, execution, port).await
        }
        ManualCompactionPhaseOutcome::TimedOut => {
            if let Some(activity_id) = activity_id {
                let _ = port.finish_activity(activity_id, ActivityTerminal::Terminated);
            }
            timeout_run(run, execution, port).await
        }
    }
}
