use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::root::TuiModel;
use crate::tui::model::runtime::workspace::WorktreeKind as ModelWorktreeKind;
use crate::tui::render::status::WorktreeKind as StatusWorktreeKind;
use crate::tui::StatusBar;

pub(crate) fn apply_runtime_status_to_widget(
    model: &TuiModel,
    last_input_tokens: u64,
    status_bar: &mut StatusBar,
) {
    status_bar.set_tokens(
        model.runtime.usage.input_tokens,
        model.runtime.usage.output_tokens,
        last_input_tokens,
    );
    if let Some(tps) = model.runtime.live_tps {
        status_bar.set_tps(tps);
    }
    if let Some(model_id) = &model.runtime.model_id {
        status_bar.set_model(model_id);
    }
    if let Some(session_id) = &model.session.current_session_id {
        status_bar.set_session_id(session_id);
    }
    if let (Some(path_base), Some(working_root)) = (
        model.runtime.workspace.path_base.clone(),
        model.runtime.workspace.working_root.clone(),
    ) {
        status_bar.set_context_paths(path_base, working_root);
    }
    status_bar.set_git_context(
        status_worktree_kind(model.runtime.workspace.kind),
        model.runtime.workspace.branch.clone().unwrap_or_default(),
    );
}

pub(crate) fn apply_diagnostic_status_to_widget(model: &TuiModel, status_bar: &mut StatusBar) {
    match model.diagnostic.highest_severity() {
        Some(DiagnosticSeverity::Error) => status_bar.set_warning("Error"),
        Some(DiagnosticSeverity::Warning) => status_bar.set_warning("Warning"),
        Some(DiagnosticSeverity::Info) | None => {}
    }
}

fn status_worktree_kind(kind: ModelWorktreeKind) -> StatusWorktreeKind {
    match kind {
        ModelWorktreeKind::MainCheckout => StatusWorktreeKind::Main,
        ModelWorktreeKind::LinkedWorktree => StatusWorktreeKind::Worktree,
        ModelWorktreeKind::Unknown => StatusWorktreeKind::Main,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::runtime::workspace::WorktreeKind;

    #[test]
    fn test_status_worktree_kind_maps_main() {
        assert_eq!(
            status_worktree_kind(WorktreeKind::MainCheckout),
            StatusWorktreeKind::Main
        );
    }

    #[test]
    fn test_status_worktree_kind_maps_linked_worktree() {
        assert_eq!(
            status_worktree_kind(WorktreeKind::LinkedWorktree),
            StatusWorktreeKind::Worktree
        );
    }

    #[test]
    fn test_status_worktree_kind_maps_unknown_to_main() {
        assert_eq!(
            status_worktree_kind(WorktreeKind::Unknown),
            StatusWorktreeKind::Main
        );
    }
}
