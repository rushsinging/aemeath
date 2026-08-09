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
        let spinner = activity_summary.map(|summary| SpinnerLineView {
            frame: activity.frame.max(anim.frame),
            verb: if activity.verb.is_empty() {
                anim.verb.clone()
            } else {
                activity.verb.clone()
            },
            elapsed_secs: activity.total_elapsed_secs(now),
            phase_elapsed_secs: activity.phase_elapsed_secs(now),
            phase_text: Some(summary.phase_text),
            detail_text: summary.detail,
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
mod tests {
    use super::*;
    use crate::tui::adapter::tui_runtime_event::{
        TuiActivityAudience, TuiActivityDetail, TuiActivityKind, TuiActivityObservation,
        TuiActivitySource, TuiActivityState, TuiActivityTiming, TuiCompactStage, TuiCompactWork,
        TuiModelStreamState, TuiRunPhaseKind, TuiRunPurpose, UiActivityId,
    };
    use crate::tui::model::conversation::intent::UpdateTaskLines;
    use crate::tui::model::conversation::interaction::UiRunId;

    fn activity(
        id: &str,
        revision: u64,
        kind: TuiActivityKind,
        detail: TuiActivityDetail,
        timing: TuiActivityTiming,
    ) -> TuiActivityObservation {
        TuiActivityObservation {
            id: UiActivityId::from(id),
            run_id: UiRunId::from("main-1"),
            run_step_id: None,
            parent_activity_id: (!matches!(kind, TuiActivityKind::Run))
                .then(|| UiActivityId::from("root")),
            source: match kind {
                TuiActivityKind::Compaction => TuiActivitySource::Compaction(UiActivityId::from(
                    format!("{id}-source").as_str(),
                )),
                _ => TuiActivitySource::Run,
            },
            kind,
            state: TuiActivityState::Running,
            detail,
            audience: TuiActivityAudience::User,
            revision,
            timing,
        }
    }

    fn conversation_with_observations(
        revision: u64,
        activities: Vec<TuiActivityObservation>,
    ) -> ConversationModel {
        let mut conversation = ConversationModel::default();
        conversation.activity_observations_mut().replace_for_test(
            UiRunId::from("main-1"),
            revision,
            activities,
        );
        conversation
    }

    fn conversation_with_activities(
        primary_kind: TuiActivityKind,
        primary_detail: TuiActivityDetail,
    ) -> ConversationModel {
        conversation_with_observations(
            2,
            vec![
                activity(
                    "root",
                    1,
                    TuiActivityKind::Run,
                    TuiActivityDetail::Run {
                        purpose: TuiRunPurpose::Main,
                    },
                    TuiActivityTiming {
                        total_elapsed_ms: 12_345,
                        active_elapsed_ms: 12_345,
                        state_elapsed_ms: 12_345,
                        ..TuiActivityTiming::default()
                    },
                ),
                activity(
                    "primary",
                    2,
                    primary_kind,
                    primary_detail,
                    TuiActivityTiming {
                        total_elapsed_ms: 6_789,
                        active_elapsed_ms: 6_789,
                        state_elapsed_ms: 6_789,
                        ..TuiActivityTiming::default()
                    },
                ),
            ],
        )
    }

    fn activity_state() -> RunActivityState {
        let mut state = RunActivityState::default();
        state.sync_main_run(
            Some(&UiRunId::from("main-1")),
            false,
            2,
            12_345,
            2,
            6_789,
            Instant::now(),
        );
        state
    }

    #[test]
    fn phase_change_resets_inner_timer_without_resetting_outer_total() {
        let anim = SpinnerAnim::default();
        let mut model = conversation_with_activities(
            TuiActivityKind::ModelInvocation,
            TuiActivityDetail::Model {
                model: "claude".to_string(),
                attempt: 1,
                stream: TuiModelStreamState::Streaming,
            },
        );
        let first_summary = ActivitySummaryAssembler::assemble(model.activity_observations())
            .expect("first activity summary");
        let now = Instant::now();
        let mut activity = RunActivityState::default();
        activity.sync_activity_summary(Some(&first_summary), now);

        let run_id = first_summary.run_id.clone();
        let mut observations = model.activity_observations().activities().to_vec();
        let root = observations
            .iter_mut()
            .find(|observation| observation.kind == TuiActivityKind::Run)
            .expect("main root");
        root.timing.total_elapsed_ms = 14_000;
        let phase = observations
            .iter_mut()
            .find(|observation| observation.kind != TuiActivityKind::Run)
            .expect("visible phase");
        phase.revision = phase.revision.saturating_add(1);
        phase.timing.state_elapsed_ms = 0;
        model
            .activity_observations_mut()
            .replace_for_test(run_id, 3, observations);
        let next_summary = ActivitySummaryAssembler::assemble(model.activity_observations())
            .expect("next activity summary");
        activity.sync_activity_summary(Some(&next_summary), now);

        let view = LiveStatusAssembler::assemble(&model, &activity, &anim, &[]);
        let spinner = view.spinner.expect("activity spinner");
        assert!(spinner.elapsed_secs >= 12);
        assert!(spinner.elapsed_secs > spinner.phase_elapsed_secs);
        assert!(spinner.phase_elapsed_secs <= 1);
    }

    #[test]
    fn activity_model_controls_visibility_and_runtime_timing() {
        let anim = SpinnerAnim::default();
        let model = conversation_with_activities(
            TuiActivityKind::ModelInvocation,
            TuiActivityDetail::Model {
                model: "claude".to_string(),
                attempt: 1,
                stream: TuiModelStreamState::Streaming,
            },
        );
        let view = LiveStatusAssembler::assemble(&model, &activity_state(), &anim, &[]);
        assert_eq!(
            view.spinner
                .as_ref()
                .and_then(|spinner| spinner.phase_text.as_deref()),
            Some("Thinking…")
        );
        let spinner = view.spinner.expect("activity spinner");
        assert_eq!(spinner.elapsed_secs, 12);
        assert_eq!(spinner.phase_elapsed_secs, 6);

        let empty = LiveStatusAssembler::assemble(
            &ConversationModel::default(),
            &RunActivityState::default(),
            &anim,
            &[],
        );
        assert!(empty.spinner.is_none());
    }

    #[test]
    fn compact_activity_drives_inline_progress_without_legacy_runtime_state() {
        let conversation = conversation_with_activities(
            TuiActivityKind::Compaction,
            TuiActivityDetail::Compact {
                stage: TuiCompactStage::Mapping,
                work: TuiCompactWork::Determinate {
                    completed: 2,
                    total: 4,
                },
            },
        );

        let view = LiveStatusAssembler::assemble(
            &conversation,
            &activity_state(),
            &SpinnerAnim::default(),
            &[],
        );

        assert_eq!(
            view.compact_progress,
            Some(crate::tui::view_model::live_status::CompactProgressView {
                ratio_millis: 375,
                stage: "mapping".to_string(),
                current: Some(2),
                total: Some(4),
            })
        );
    }

    #[test]
    fn phase_matrix_produces_expected_activity_text() {
        let cases = [
            (TuiRunPhaseKind::DrainingInput, "Preparing input…"),
            (TuiRunPhaseKind::PreparingContext, "Preparing context…"),
            (TuiRunPhaseKind::ApplyingResponse, "Applying response…"),
            (
                TuiRunPhaseKind::AwaitingToolApproval,
                "Waiting for approval…",
            ),
            (TuiRunPhaseKind::ExecutingTools, "Calling tools…"),
            (TuiRunPhaseKind::CancellingStep, "Cancelling step…"),
            (TuiRunPhaseKind::FinalizingStep, "Finalizing step…"),
            (TuiRunPhaseKind::Terminating, "Terminating…"),
        ];
        for (phase, expected) in cases {
            let model = conversation_with_activities(
                TuiActivityKind::RunPhase(phase),
                TuiActivityDetail::Phase { phase },
            );
            let view = LiveStatusAssembler::assemble(
                &model,
                &activity_state(),
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
    fn user_tool_detail_is_gated_and_parallel_tools_use_stable_summary() {
        let root = activity(
            "root",
            1,
            TuiActivityKind::Run,
            TuiActivityDetail::Run {
                purpose: TuiRunPurpose::Main,
            },
            TuiActivityTiming {
                total_elapsed_ms: 2_000,
                state_elapsed_ms: 2_000,
                ..TuiActivityTiming::default()
            },
        );
        let tool = |revision, elapsed_ms, parallel_count| {
            activity(
                "tool",
                revision,
                TuiActivityKind::ToolCall,
                TuiActivityDetail::Tool {
                    name: "Read".to_string(),
                    summary: Some("src/runtime.rs".to_string()),
                    parallel_count,
                },
                TuiActivityTiming {
                    total_elapsed_ms: elapsed_ms,
                    state_elapsed_ms: elapsed_ms,
                    ..TuiActivityTiming::default()
                },
            )
        };

        let short = conversation_with_observations(2, vec![root.clone(), tool(2, 499, 1)]);
        assert_eq!(
            LiveStatusAssembler::assemble(&short, &activity_state(), &SpinnerAnim::default(), &[],)
                .spinner
                .and_then(|spinner| spinner.detail_text),
            None
        );

        let single = conversation_with_observations(2, vec![root.clone(), tool(2, 500, 1)]);
        assert_eq!(
            LiveStatusAssembler::assemble(
                &single,
                &activity_state(),
                &SpinnerAnim::default(),
                &[],
            )
            .spinner
            .and_then(|spinner| spinner.detail_text),
            Some("Read src/runtime.rs".to_string())
        );

        let parallel = conversation_with_observations(2, vec![root, tool(2, 500, 3)]);
        assert_eq!(
            LiveStatusAssembler::assemble(
                &parallel,
                &activity_state(),
                &SpinnerAnim::default(),
                &[],
            )
            .spinner
            .and_then(|spinner| spinner.detail_text),
            Some("Running 3 tools".to_string())
        );
    }

    #[test]
    fn diagnostic_leaf_never_enters_default_summary() {
        let mut hook = activity(
            "hook",
            2,
            TuiActivityKind::HookDispatch,
            TuiActivityDetail::Hook {
                point: crate::tui::adapter::tui_runtime_event::TuiHookPoint::Stop,
                script: "check-stop.sh".to_string(),
                attempt: 1,
            },
            TuiActivityTiming {
                total_elapsed_ms: 5_000,
                state_elapsed_ms: 5_000,
                ..TuiActivityTiming::default()
            },
        );
        hook.audience = TuiActivityAudience::Diagnostic;
        let model = conversation_with_observations(
            2,
            vec![
                activity(
                    "root",
                    1,
                    TuiActivityKind::Run,
                    TuiActivityDetail::Run {
                        purpose: TuiRunPurpose::Main,
                    },
                    TuiActivityTiming::default(),
                ),
                hook,
            ],
        );

        assert!(LiveStatusAssembler::assemble(
            &model,
            &activity_state(),
            &SpinnerAnim::default(),
            &[],
        )
        .spinner
        .is_none());
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
