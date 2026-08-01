//! Live status adapter 已退役：spinner/task/queued 不再写回 `OutputArea` 镜像。
//!
//! 生产路径：`App::refresh_live_status_from_model()` 仅维护 `view_state.spinner` 的
//! verb/frame 生命周期；`App::live_status_view_model()` 从 Model + view_state 派生
//! `LiveStatusViewModel`；`OutputArea::render(...)` 直接消费该 ViewModel。

#[cfg(test)]
mod tests {
    use crate::tui::adapter::tui_runtime_event::{
        TuiActivityAudience, TuiActivityDetail, TuiActivityKind, TuiActivityObservation,
        TuiActivitySource, TuiActivityState, TuiActivityTiming, TuiModelStreamState, TuiRunPurpose,
        UiActivityId,
    };
    use crate::tui::model::conversation::intent::UpdateTaskLines;
    use crate::tui::model::conversation::interaction::UiRunId;
    use crate::tui::model::conversation::model::ConversationModel;
    use crate::tui::view_assembler::live_status::LiveStatusAssembler;
    use crate::tui::view_state::{RunActivityState, SpinnerAnim};

    #[test]
    fn live_status_projection_includes_spinner_task_and_queued_lines() {
        let mut model = ConversationModel::default();
        let run_id = UiRunId::from("main-1");
        model.activity_observations_mut().replace_for_test(
            run_id.clone(),
            2,
            vec![
                TuiActivityObservation {
                    id: UiActivityId::from("root"),
                    run_id: run_id.clone(),
                    run_step_id: None,
                    parent_activity_id: None,
                    source: TuiActivitySource::Run,
                    kind: TuiActivityKind::Run,
                    state: TuiActivityState::Running,
                    detail: TuiActivityDetail::Run {
                        purpose: TuiRunPurpose::Main,
                    },
                    audience: TuiActivityAudience::User,
                    revision: 1,
                    timing: TuiActivityTiming {
                        total_elapsed_ms: 12_345,
                        active_elapsed_ms: 12_345,
                        state_elapsed_ms: 12_345,
                        started_at_unix_ms: None,
                        finished_at_unix_ms: None,
                    },
                },
                TuiActivityObservation {
                    id: UiActivityId::from("model"),
                    run_id: run_id.clone(),
                    run_step_id: None,
                    parent_activity_id: Some(UiActivityId::from("root")),
                    source: TuiActivitySource::Run,
                    kind: TuiActivityKind::ModelInvocation,
                    state: TuiActivityState::Running,
                    detail: TuiActivityDetail::Model {
                        model: "claude".to_string(),
                        attempt: 1,
                        stream: TuiModelStreamState::Streaming,
                    },
                    audience: TuiActivityAudience::User,
                    revision: 2,
                    timing: TuiActivityTiming {
                        total_elapsed_ms: 678,
                        active_elapsed_ms: 678,
                        state_elapsed_ms: 678,
                        started_at_unix_ms: None,
                        finished_at_unix_ms: None,
                    },
                },
            ],
        );
        model.apply(UpdateTaskLines(vec![
            "━━ Tasks: 1/2 ━━".to_string(),
            "✓ #1 done".to_string(),
        ]));
        let anim = SpinnerAnim {
            frame: 12,
            phase_frame: 4,
            verb: "Forging".to_string(),
        };
        let queued = vec!["queued input".to_string()];

        let now = std::time::Instant::now();
        let mut activity = RunActivityState::default();
        activity.sync_main_run(Some(&run_id), true, 2, 12_345, 678, now);
        let vm = LiveStatusAssembler::assemble(&model, &activity, &anim, &queued);
        let spinner = vm.spinner.expect("spinner projected");
        assert_eq!(spinner.frame, 12);
        assert_eq!(spinner.verb, "Forging");
        assert_eq!(spinner.elapsed_secs, 12);
        assert_eq!(spinner.phase_elapsed_secs, 0);
        assert_eq!(spinner.phase_text.as_deref(), Some("Thinking…"));
        assert_eq!(vm.task_lines, vec!["━━ Tasks: 1/2 ━━", "✓ #1 done"]);
        assert_eq!(vm.queued_lines, vec!["> queued input"]);
    }
}
