use crate::tui::core::state::{ChatState, InputState, SessionState, UiLayout};
use crate::tui::model::input::attachment::InputAttachment;
use crate::tui::model::input::completion_item::CompletionItem;
use crate::tui::model::root::TuiModel;
use crate::tui::model::runtime::workspace::WorktreeKind as ModelWorktreeKind;
use crate::tui::model::session::resume::SessionResumeCandidate;
use crate::tui::view_state::output::{ScreenLineMapEntry, SelectedTextRange};
use crate::tui::view_state::AppViewState;
use crate::tui::{InputArea, OutputArea};

pub(crate) fn sync_model_from_legacy(
    model: &mut TuiModel,
    chat: &ChatState,
    input_state: &InputState,
    input_area: &InputArea,
    session: &SessionState,
    _layout: &UiLayout,
    output_area: &OutputArea,
) {
    model.session.current_session_id = Some(session.session_id.clone());
    model.session.message_count = chat.messages.len();
    model.session.resume_candidates = session
        .cached_sessions
        .iter()
        .map(|(id, title)| SessionResumeCandidate::new(id, title))
        .collect();

    model.runtime.model_id = Some(session.current_model_display.clone());
    model.runtime.workspace.cwd = Some(session.cwd.display().to_string());
    let usage = chat.usage_snapshot();
    model.runtime.usage.input_tokens = usage.total_input_tokens;
    model.runtime.usage.output_tokens = usage.total_output_tokens;
    model.runtime.processing_jobs.clear();
    if chat.is_processing {
        model.runtime.processing_jobs.push(
            crate::tui::model::runtime::processing_job::ProcessingJob {
                id: "legacy-chat".to_string(),
                chat_id: model
                    .conversation
                    .active_chat_id
                    .as_ref()
                    .map(|id| id.as_ref().to_string()),
                status: crate::tui::model::runtime::processing_job::ProcessingStatus::Running,
            },
        );
    }

    model.input.document.clear();
    model.input.document.insert_text(&input_area.get_text());
    let (_, col) = input_area.cursor_position();
    model.input.document.move_cursor(col);
    let (history_entries, history_index, saved_input) = input_area.history_snapshot();
    model.input.history.entries = history_entries.to_vec();
    model.input.history.selected_index = history_index;
    model.input.history.saved_input = saved_input.to_string();
    let (suggestions, selected_suggestion, show_suggestions) = input_area.suggestions_snapshot();
    model.input.completion.visible = show_suggestions;
    model.input.completion.selected_index = selected_suggestion;
    model.input.completion.items = suggestions
        .iter()
        .map(|suggestion| CompletionItem::new(&suggestion.display_text, &suggestion.display_text))
        .collect();
    model.input.attachments = chat
        .pending_images()
        .iter()
        .enumerate()
        .map(|(index, _)| InputAttachment {
            label: format!("image-{}", index + 1),
            path: None,
        })
        .collect();
    model.input.mode = if show_suggestions {
        crate::tui::model::input::mode::InputMode::Completion
    } else if input_state.ask_user_state.is_some() {
        crate::tui::model::input::mode::InputMode::PromptAnswer
    } else {
        crate::tui::model::input::mode::InputMode::Normal
    };

    model.runtime.workspace.kind = ModelWorktreeKind::Unknown;
    model.diagnostic.active_prompt = None;
    model.diagnostic.notices.clear();
    model.conversation.queued_submissions = output_area
        .queued_messages
        .iter()
        .enumerate()
        .map(
            |(index, text)| crate::tui::model::conversation::queued_submission::QueuedSubmission {
                id: format!("legacy-queued-{}", index + 1),
                text: text.clone(),
            },
        )
        .collect();
}

pub(crate) fn sync_view_state_from_legacy(
    view_state: &mut AppViewState,
    layout: &UiLayout,
    output_area: &OutputArea,
) {
    view_state.layout.terminal_width = layout.output_area_rect.width;
    view_state.layout.terminal_height = layout.output_area_rect.height;
    view_state.layout.version = view_state.layout.version.saturating_add(1);
    view_state.output.scroll_offset = output_area.scroll_offset;
    view_state.output.follow_tail = output_area.auto_scroll;
    view_state.output.auto_scroll = output_area.auto_scroll;
    view_state.output.is_selecting = output_area.is_selecting;
    view_state.output.selection_start =
        output_area
            .selection_start
            .as_ref()
            .map(|(line, offset)| SelectedTextRange {
                start_block_key: line.to_string(),
                start_offset: offset.as_usize(),
                end_block_key: line.to_string(),
                end_offset: offset.as_usize(),
            });
    view_state.output.selection_end =
        output_area
            .selection_end
            .as_ref()
            .map(|(line, offset)| SelectedTextRange {
                start_block_key: line.to_string(),
                start_offset: offset.as_usize(),
                end_block_key: line.to_string(),
                end_offset: offset.as_usize(),
            });
    view_state.output.screen_line_map = output_area
        .screen_line_map
        .iter()
        .map(|(line_index, start, _)| ScreenLineMapEntry {
            block_key: line_index.to_string(),
            line_index: start.as_usize(),
        })
        .collect();
    view_state.output.last_visible_height = output_area.last_visible_height;
    view_state.output.version = view_state.output.version.saturating_add(1);
}

#[cfg(test)]
pub(crate) fn collect_mismatches(
    actual_model: &TuiModel,
    expected_model: &TuiModel,
    actual_view_state: &AppViewState,
    expected_view_state: &AppViewState,
) -> Vec<String> {
    let mut mismatches = Vec::new();
    if actual_model.session.current_session_id != expected_model.session.current_session_id {
        mismatches.push("session.current_session_id".to_string());
    }
    if actual_model.session.message_count != expected_model.session.message_count {
        mismatches.push("session.message_count".to_string());
    }
    if actual_model.runtime.model_id != expected_model.runtime.model_id {
        mismatches.push("runtime.model_id".to_string());
    }
    if actual_model.runtime.usage != expected_model.runtime.usage {
        mismatches.push("runtime.usage".to_string());
    }
    if actual_model.input.document != expected_model.input.document {
        mismatches.push("input.document".to_string());
    }
    if actual_model.input.completion != expected_model.input.completion {
        mismatches.push("input.completion".to_string());
    }
    if actual_model.input.attachments != expected_model.input.attachments {
        mismatches.push("input.attachments".to_string());
    }
    if actual_model.conversation.queued_submissions
        != expected_model.conversation.queued_submissions
    {
        mismatches.push("conversation.queued_submissions".to_string());
    }
    if actual_view_state.output.scroll_offset != expected_view_state.output.scroll_offset {
        mismatches.push("view_state.output.scroll_offset".to_string());
    }
    if actual_view_state.output.auto_scroll != expected_view_state.output.auto_scroll {
        mismatches.push("view_state.output.auto_scroll".to_string());
    }
    mismatches
}
