//! 由 typed Main Run snapshot + view_state 纯动画态派生 `LiveStatusViewModel`。
//! Run status 到展示文案的转换集中在此；旧 spinner 业务生命周期不参与组装。
//!
//! 本层可依赖 model（边界守卫只禁渲染库/副作用），但 ViewModel 输出仅含基本类型。

use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::view_assembler::activity_summary::ActivitySummaryAssembler;
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
        let activity_summary =
            ActivitySummaryAssembler::assemble(conversation.activity_observations());
        let compact_progress = activity_summary
            .as_ref()
            .and_then(|summary| compact_progress_from_activity(conversation, &summary.run_id));
        let spinner = activity_summary.map(|summary| {
            let primary = summary.primary;
            SpinnerLineView {
                frame: activity.frame.max(anim.frame),
                verb: if activity.verb.is_empty() {
                    anim.verb.clone()
                } else {
                    activity.verb.clone()
                },
                elapsed_secs: activity.total_elapsed_secs(now),
                phase_elapsed_secs: primary.as_ref().map(|_| activity.phase_elapsed_secs(now)),
                phase_text: primary.as_ref().map(|primary| primary.phase_text.clone()),
                detail_text: primary.and_then(|primary| primary.detail),
            }
        });
        let queued_lines = queued_texts
            .iter()
            .flat_map(|text| queued_preview_lines(text))
            .collect();
        LiveStatusViewModel {
            spinner,
            queued_lines,
            task_lines: conversation.runtime.task_status.lines.clone(),
            compact_progress,
            background_processes_active: conversation.runtime.background_processes_active,
        }
    }
}

fn compact_progress_from_activity(
    conversation: &ConversationModel,
    run_id: &crate::tui::model::conversation::interaction::UiRunId,
) -> Option<crate::tui::view_model::live_status::CompactProgressView> {
    use crate::tui::adapter::tui_runtime_event::{
        TuiActivityDetail, TuiActivityKind, TuiActivityState, TuiCompactStage, TuiCompactWork,
    };

    let activity = conversation
        .activity_observations()
        .activities()
        .iter()
        .filter(|activity| {
            activity.run_id == *run_id
                && activity.kind == TuiActivityKind::Compaction
                && matches!(
                    activity.state,
                    TuiActivityState::Running | TuiActivityState::Waiting
                )
        })
        .max_by_key(|activity| activity.revision)?;
    let TuiActivityDetail::Compact { stage, work } = activity.detail else {
        return None;
    };
    let (stage, ratio_millis) = match stage {
        TuiCompactStage::Preparing => ("preparing", 50),
        TuiCompactStage::Generating => ("generating", 300),
        TuiCompactStage::Mapping => {
            let ratio_millis = match work {
                TuiCompactWork::Determinate { completed, total } if total > 0 => {
                    150u32.saturating_add(450u32.saturating_mul(completed) / total)
                }
                _ => 350,
            };
            ("mapping", ratio_millis)
        }
        TuiCompactStage::Reducing => ("reducing", 700),
        TuiCompactStage::Refreshing => {
            let ratio_millis = match work {
                TuiCompactWork::Determinate { completed, total } if total > 0 => {
                    750u32.saturating_add(100u32.saturating_mul(completed) / total)
                }
                _ => 800,
            };
            ("refreshing", ratio_millis)
        }
        TuiCompactStage::Finalizing => ("finalizing", 900),
    };
    let (current, total) = match work {
        TuiCompactWork::Indeterminate => (None, None),
        TuiCompactWork::Determinate { completed, total } => (Some(completed), Some(total)),
    };

    Some(crate::tui::view_model::live_status::CompactProgressView {
        ratio_millis: ratio_millis.min(1_000),
        stage: stage.to_string(),
        current,
        total,
    })
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
#[path = "live_status_tests.rs"]
mod tests;
