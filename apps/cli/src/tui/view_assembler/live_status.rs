//! 由 typed Main Run snapshot + view_state 纯动画态派生 `LiveStatusViewModel`。
//! Run status 到展示文案的转换集中在此；旧 spinner 业务生命周期不参与组装。
//!
//! 本层可依赖 model（边界守卫只禁渲染库/副作用），但 ViewModel 输出仅含基本类型。

use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::run_state::is_terminal;
use crate::tui::view_model::{LiveStatusViewModel, SpinnerLineView};
use crate::tui::view_state::{RunActivityState, SpinnerAnim};
use std::time::Instant;

pub struct LiveStatusAssembler;

impl LiveStatusAssembler {
    /// 由 Model 业务态 + view_state 动画态 + 排队输入派生实时状态行视图。
    ///
    /// 排队输入真相目前归 `ConversationModel::queued_submissions`；调用方只传入文本切片，
    /// 本层负责统一格式化为 live-status 预览行，避免 OutputArea 自持排队状态。
    pub fn assemble(
        conversation: &ConversationModel,
        activity: &RunActivityState,
        anim: &SpinnerAnim,
        queued_texts: &[String],
    ) -> LiveStatusViewModel {
        let now = Instant::now();
        let spinner = conversation
            .active_main_run_snapshot()
            .filter(|snapshot| !is_terminal(snapshot.status))
            .and_then(|snapshot| {
                let phase_text = match snapshot.status {
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::Created
                    | crate::tui::adapter::tui_runtime_event::TuiRunStatus::AwaitingToolApproval
                    | crate::tui::adapter::tui_runtime_event::TuiRunStatus::AwaitingUser => None,
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::DrainingInput => {
                        Some("Preparing input…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::PreparingContext => {
                        Some("Preparing context…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::InvokingModel => {
                        Some("Thinking…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::ApplyingResponse => {
                        Some("Applying response…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::ExecutingTools => {
                        Some("Calling tools…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::Compacting => {
                        Some("Compacting…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::CancellingStep => {
                        Some("Cancelling step…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::FinalizingStep => {
                        Some("Finalizing step…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::Cancelling => {
                        Some("Cancelling…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::Terminating => {
                        Some("Terminating…".to_string())
                    }
                    crate::tui::adapter::tui_runtime_event::TuiRunStatus::Completed
                    | crate::tui::adapter::tui_runtime_event::TuiRunStatus::Failed
                    | crate::tui::adapter::tui_runtime_event::TuiRunStatus::Cancelled
                    | crate::tui::adapter::tui_runtime_event::TuiRunStatus::Terminated => None,
                }?;
                Some(SpinnerLineView {
                    frame: activity.frame.max(anim.frame),
                    verb: if activity.verb.is_empty() {
                        anim.verb.clone()
                    } else {
                        activity.verb.clone()
                    },
                    elapsed_secs: activity.total_elapsed_secs(now),
                    phase_elapsed_secs: activity.phase_elapsed_secs(now),
                    phase_text: Some(phase_text),
                })
            });
        let queued_lines = queued_texts
            .iter()
            .flat_map(|text| queued_preview_lines(text))
            .collect();
        let compact_progress = conversation.runtime.compact_progress.as_ref().map(|p| {
            use crate::tui::view_model::live_status::CompactProgressView;
            let ratio = p.ratio().clamp(0.0, 1.0);
            CompactProgressView {
                ratio_millis: (ratio * 1000.0).round() as u32,
                stage: p.stage.clone(),
                current: p.current,
                total: p.total,
            }
        });
        LiveStatusViewModel {
            spinner,
            queued_lines,
            task_lines: conversation.runtime.task_status.lines.clone(),
            compact_progress,
        }
    }
}

fn queued_preview_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for (idx, line) in text.split('\n').enumerate() {
        let prefix = if idx == 0 { "> " } else { "  " };
        lines.push(format!("{prefix}{line}"));
    }
    if lines.is_empty() {
        lines.push("> ".to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::adapter::tui_runtime_event::TuiRunStatus;
    use crate::tui::model::conversation::intent::{ObserveRunStatus, UpdateTaskLines};
    use crate::tui::model::conversation::interaction::UiRunId;

    fn conversation_at(status: TuiRunStatus) -> ConversationModel {
        let mut conversation = ConversationModel::default();
        conversation.apply(ObserveRunStatus {
            run_id: UiRunId::from("main-1"),
            parent_run_id: None,
            status,
            timing: crate::tui::adapter::tui_runtime_event::TuiRunTiming {
                observation_revision: 1,
                total_elapsed_ms: 12_345,
                phase_elapsed_ms: 6_789,
            },
        });
        conversation
    }

    #[test]
    fn typed_status_controls_activity_visibility() {
        let anim = SpinnerAnim::default();
        let mut activity = RunActivityState::default();
        activity.sync_main_run(
            Some(&UiRunId::from("main-1")),
            true,
            1,
            12_345,
            6_789,
            Instant::now(),
        );
        let invoking = LiveStatusAssembler::assemble(
            &conversation_at(TuiRunStatus::InvokingModel),
            &activity,
            &anim,
            &[],
        );
        assert_eq!(
            invoking
                .spinner
                .as_ref()
                .and_then(|view| view.phase_text.clone()),
            Some("Thinking…".to_string())
        );
        let spinner = invoking.spinner.expect("runtime-timed activity");
        assert_eq!(spinner.elapsed_secs, 12);
        assert_eq!(spinner.phase_elapsed_secs, 6);

        for status in [
            TuiRunStatus::Created,
            TuiRunStatus::AwaitingToolApproval,
            TuiRunStatus::AwaitingUser,
            TuiRunStatus::Completed,
            TuiRunStatus::Failed,
            TuiRunStatus::Cancelled,
            TuiRunStatus::Terminated,
        ] {
            let view =
                LiveStatusAssembler::assemble(&conversation_at(status), &activity, &anim, &[]);
            assert!(view.spinner.is_none(), "{status:?} must not show activity");
        }
    }

    #[test]
    fn status_matrix_produces_expected_activity_text() {
        let cases = [
            (TuiRunStatus::DrainingInput, "Preparing input…"),
            (TuiRunStatus::PreparingContext, "Preparing context…"),
            (TuiRunStatus::Compacting, "Compacting…"),
            (TuiRunStatus::ApplyingResponse, "Applying response…"),
            (TuiRunStatus::ExecutingTools, "Calling tools…"),
            (TuiRunStatus::CancellingStep, "Cancelling step…"),
            (TuiRunStatus::FinalizingStep, "Finalizing step…"),
            (TuiRunStatus::Cancelling, "Cancelling…"),
            (TuiRunStatus::Terminating, "Terminating…"),
        ];
        for (status, expected) in cases {
            let view = LiveStatusAssembler::assemble(
                &conversation_at(status),
                &RunActivityState::default(),
                &SpinnerAnim::default(),
                &[],
            );
            assert_eq!(
                view.spinner.and_then(|activity| activity.phase_text),
                Some(expected.to_string())
            );
        }
    }

    #[test]
    fn queued_and_task_lines_remain_independent_of_run_activity() {
        let mut conversation = ConversationModel::default();
        conversation.apply(UpdateTaskLines(vec!["□ task".to_string()]));
        let view = LiveStatusAssembler::assemble(
            &conversation,
            &RunActivityState::default(),
            &SpinnerAnim::default(),
            &["alpha\nbeta".to_string()],
        );
        assert_eq!(view.queued_lines, vec!["> alpha", "  beta"]);
        assert_eq!(view.task_lines, vec!["□ task"]);
    }
}
