#[cfg(test)]
use crate::tui::core::event::{StatusContextUpdate, UiEvent};
use crate::tui::display::status_bar::WorktreeKind as StatusWorktreeKind;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::root::TuiModel;
use crate::tui::model::runtime::workspace::WorktreeKind as ModelWorktreeKind;
use crate::tui::{core::state::ChatState, StatusBar};

pub(crate) fn apply_runtime_status_to_legacy(
    model: &TuiModel,
    chat: &ChatState,
    status_bar: &mut StatusBar,
) {
    status_bar.set_tokens(
        model.runtime.usage.input_tokens,
        model.runtime.usage.output_tokens,
        chat.last_input_tokens,
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

pub(crate) fn apply_diagnostic_status_to_legacy(model: &TuiModel, status_bar: &mut StatusBar) {
    match model.diagnostic.highest_severity() {
        Some(DiagnosticSeverity::Error) => status_bar.set_warning("Error"),
        Some(DiagnosticSeverity::Warning) => status_bar.set_warning("Warning"),
        Some(DiagnosticSeverity::Info) | None => {}
    }
}

#[cfg(test)]
pub(crate) fn status_context_to_model_kind(update: &StatusContextUpdate) -> ModelWorktreeKind {
    status_worktree_to_model(update.kind)
}

#[cfg(test)]
pub(crate) fn runtime_event_tps(event: &UiEvent) -> Option<f64> {
    match event {
        UiEvent::Usage {
            output,
            elapsed_secs,
            ..
        } => Some(if *elapsed_secs > 0.0 {
            *output as f64 / *elapsed_secs
        } else {
            0.0
        }),
        UiEvent::LiveTps(tps) => Some(*tps),
        _ => None,
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
fn status_worktree_to_model(kind: StatusWorktreeKind) -> ModelWorktreeKind {
    match kind {
        StatusWorktreeKind::Main => ModelWorktreeKind::MainCheckout,
        StatusWorktreeKind::Worktree => ModelWorktreeKind::LinkedWorktree,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::core::event::UiEvent;

    #[test]
    fn test_runtime_event_tps_from_usage() {
        let tps = runtime_event_tps(&UiEvent::Usage {
            input: 1,
            output: 10,
            last_input: 1,
            elapsed_secs: 2.0,
        });

        assert_eq!(tps, Some(5.0));
    }

    #[test]
    fn test_runtime_event_tps_zero_elapsed() {
        let tps = runtime_event_tps(&UiEvent::Usage {
            input: 1,
            output: 10,
            last_input: 1,
            elapsed_secs: 0.0,
        });

        assert_eq!(tps, Some(0.0));
    }

    #[test]
    fn test_status_context_to_model_kind_maps_worktree() {
        let update = StatusContextUpdate {
            path_base: "/repo".to_string(),
            working_root: "/repo".to_string(),
            raw_path_base: std::path::PathBuf::from("/repo"),
            raw_working_root: std::path::PathBuf::from("/repo"),
            workspace: sdk::WorkspaceContextView {
                path_base: std::path::PathBuf::from("/repo"),
                working_root: std::path::PathBuf::from("/repo"),
                context_stack: Vec::new(),
            },
            branch: None,
            kind: StatusWorktreeKind::Worktree,
        };

        assert_eq!(
            status_context_to_model_kind(&update),
            ModelWorktreeKind::LinkedWorktree
        );
    }
}
