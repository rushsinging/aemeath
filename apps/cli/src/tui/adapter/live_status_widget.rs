//! Live status adapter 已退役：spinner/task/queued 不再写回 `OutputArea` 镜像。
//!
//! 生产路径：`App::refresh_live_status_from_model()` 仅维护 `view_state.spinner` 的
//! verb/frame 生命周期；`App::live_status_view_model()` 从 Model + view_state 派生
//! `LiveStatusViewModel`；`OutputArea::render(...)` 直接消费该 ViewModel。

#[cfg(test)]
mod tests {
    use crate::tui::model::conversation::intent::*;
    use crate::tui::model::conversation::model::ConversationModel;
    use crate::tui::model::conversation::spinner::SpinnerPhase;
    use crate::tui::view_assembler::live_status::LiveStatusAssembler;
    use crate::tui::view_state::SpinnerAnim;

    #[test]
    fn live_status_projection_includes_spinner_task_and_queued_lines() {
        let mut model = ConversationModel::default();
        model.spinner.phase = Some(SpinnerPhase::Thinking);
        model.apply(UpdateTaskLines(vec![
            "━━ Tasks: 1/2 ━━".to_string(),
            "✓ #1 done".to_string(),
        ]));
        let anim = SpinnerAnim {
            frame: 12,
            phase_frame: 4,
            phase: Some(SpinnerPhase::Thinking),
            verb: "Forging".to_string(),
        };
        let queued = vec!["queued input".to_string()];

        let vm = LiveStatusAssembler::assemble(&model, &anim, &queued);

        let spinner = vm.spinner.expect("spinner projected");
        assert_eq!(spinner.frame, 12);
        assert_eq!(spinner.verb, "Forging");
        assert_eq!(spinner.elapsed_secs, 1);
        assert_eq!(spinner.phase_elapsed_secs, 0);
        assert_eq!(spinner.phase_text.as_deref(), Some("Thinking..."));
        assert_eq!(vm.task_lines, vec!["━━ Tasks: 1/2 ━━", "✓ #1 done"]);
        assert_eq!(vm.queued_lines, vec!["> queued input"]);
    }
}
