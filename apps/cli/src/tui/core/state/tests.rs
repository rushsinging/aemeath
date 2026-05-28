#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use crate::tui::core::state::{ChatState, InputState, SessionState, UiLayout};
    use crate::tui::core::App;

    fn make_memory_config() -> sdk::MemoryConfigView {
        sdk::MemoryConfigView::default()
    }

    fn make_clipboard_image(size: usize) -> sdk::ClipboardImageView {
        sdk::ClipboardImageView {
            base64: "img".to_string(),
            media_type: "image/png".to_string(),
            final_size: size,
            display_path: None,
            width: None,
            height: None,
        }
    }

    // === ChatState ===

    #[test]
    fn test_chat_state_default_messages_empty() {
        let state = ChatState::default();
        assert!(state.messages.is_empty());
    }

    #[test]
    fn test_chat_state_default_not_processing() {
        let state = ChatState::default();
        assert!(!state.is_processing);
    }

    #[test]
    fn test_chat_state_default_context_size() {
        let state = ChatState::default();
        assert_eq!(state.context_size, 200_000);
    }

    #[test]
    fn test_chat_state_pending_images_add_and_count() {
        let mut state = ChatState::default();
        let count = state.add_pending_image(make_clipboard_image(12));
        assert_eq!(count, 1);
        assert_eq!(state.pending_image_count(), 1);
        assert_eq!(state.pending_images()[0].final_size, 12);
    }

    #[test]
    fn test_chat_state_pending_images_drain_clears() {
        let mut state = ChatState::default();
        state.add_pending_image(make_clipboard_image(7));
        let drained = state.drain_pending_images();
        assert_eq!(drained.len(), 1);
        assert_eq!(state.pending_image_count(), 0);
    }

    #[test]
    fn test_chat_state_usage_snapshot_after_record_usage() {
        let mut state = ChatState::default();
        state.record_usage(10, 4, 7);
        let usage = state.usage_snapshot();
        assert_eq!(usage.total_input_tokens, 10);
        assert_eq!(usage.total_output_tokens, 4);
        assert_eq!(usage.last_input_tokens, 7);
        assert_eq!(usage.total_api_calls, 1);
    }

    // === InputState ===

    #[test]
    fn test_input_state_default_queue_empty() {
        let state = InputState::default();
        assert!(state.input_queue.is_empty());
        assert!(!state.just_pasted);
        assert!(state.ask_user_state.is_none());
    }

    // === SessionState ===

    #[test]
    fn test_session_state_holds_session_id() {
        let state = SessionState {
            session_id: "sess-1".into(),
            cwd: std::path::PathBuf::from("/tmp"),
            session_created_at: None,
            cached_sessions: vec![],
            current_model_display: "gpt-4".into(),
            memory_config: make_memory_config(),
        };
        assert_eq!(state.session_id, "sess-1");
        assert_eq!(state.current_model_display, "gpt-4");
    }

    #[test]
    fn test_session_state_cached_sessions_default_empty() {
        let state = SessionState {
            session_id: "sess-1".into(),
            cwd: std::path::PathBuf::from("/tmp"),
            session_created_at: None,
            cached_sessions: vec![],
            current_model_display: "".into(),
            memory_config: make_memory_config(),
        };
        assert!(state.cached_sessions().is_empty());
    }

    #[test]
    fn test_session_state_cache_sessions_replaces_entries() {
        let mut state = SessionState {
            session_id: "sess-1".into(),
            cwd: std::path::PathBuf::from("/tmp"),
            session_created_at: None,
            cached_sessions: vec![],
            current_model_display: "".into(),
            memory_config: make_memory_config(),
        };
        state.cache_sessions(vec![("s2".to_string(), "summary".to_string())]);
        assert_eq!(
            state.cached_sessions(),
            &[("s2".to_string(), "summary".to_string())]
        );
    }

    #[test]
    fn test_session_state_rename_session_updates_id() {
        let mut state = SessionState {
            session_id: "sess-1".into(),
            cwd: std::path::PathBuf::from("/tmp"),
            session_created_at: None,
            cached_sessions: vec![],
            current_model_display: "".into(),
            memory_config: make_memory_config(),
        };
        state.rename_session("sess-2");
        assert_eq!(state.session_id(), "sess-2");
    }

    // === UiLayout ===

    #[test]
    fn test_ui_layout_default_not_exiting() {
        let layout = UiLayout::default();
        assert!(!layout.should_exit);
    }

    #[test]
    fn test_ui_layout_default_no_dialog() {
        let layout = UiLayout::default();
        assert!(layout.active_dialog.is_none());
        assert!(layout.dialog_model_keys.is_empty());
    }

    #[test]
    fn test_ui_layout_default_no_ctrlc() {
        let layout = UiLayout::default();
        assert!(layout.last_ctrlc.is_none());
    }

    #[test]
    fn test_ui_layout_request_exit_marks_exit() {
        let mut layout = UiLayout::default();
        layout.request_exit();
        assert!(layout.should_exit);
    }

    #[test]
    fn test_ui_layout_ctrlc_mark_and_clear() {
        let mut layout = UiLayout::default();
        layout.mark_ctrlc_now();
        assert!(layout.last_ctrlc.is_some());
        layout.clear_ctrlc();
        assert!(layout.last_ctrlc.is_none());
    }

    #[test]
    fn test_ui_layout_update_areas_replaces_rects() {
        let mut layout = UiLayout::default();
        let output = ratatui::layout::Rect::new(1, 2, 3, 4);
        let input = ratatui::layout::Rect::new(5, 6, 7, 8);
        let status = ratatui::layout::Rect::new(9, 10, 11, 12);
        layout.update_areas(output, input, status);
        assert_eq!(layout.output_area_rect, output);
        assert_eq!(layout.input_area_rect, input);
        assert_eq!(layout.status_bar_rect, status);
    }

    #[test]
    fn test_app_dual_track_sync_initializes_session_runtime_and_input() {
        let cwd = std::path::PathBuf::from("/tmp/aemeath");
        let app = App::new("sess-1".to_string(), cwd.clone(), "gpt-test".to_string());

        assert_eq!(
            app.model.session.current_session_id.as_deref(),
            Some("sess-1")
        );
        assert_eq!(app.model.runtime.model_id.as_deref(), Some("gpt-test"));
        assert_eq!(
            app.model.runtime.workspace.cwd,
            Some(cwd.display().to_string())
        );
        assert!(app.model.input.document.buffer.is_empty());
        assert!(app.dual_track_mismatches().is_empty());
    }

    #[test]
    fn test_app_dual_track_sync_tracks_usage_messages_and_attachments() {
        let mut app = App::new(
            "sess-usage".to_string(),
            std::path::PathBuf::from("/tmp/aemeath"),
            "gpt-test".to_string(),
        );
        app.chat.messages.push(sdk::ChatMessage::user_text("hi"));
        app.chat.record_usage(12, 7, 12);
        app.chat.add_pending_image(make_clipboard_image(42));

        app.sync_dual_track_state();

        assert_eq!(app.model.session.message_count, 1);
        assert_eq!(app.model.runtime.usage.input_tokens, 12);
        assert_eq!(app.model.runtime.usage.output_tokens, 7);
        assert_eq!(app.model.input.attachments.len(), 1);
        assert!(app.dual_track_mismatches().is_empty());
    }

    #[test]
    fn test_app_dual_track_mismatches_detect_stale_model() {
        let mut app = App::new(
            "sess-stale".to_string(),
            std::path::PathBuf::from("/tmp/aemeath"),
            "gpt-test".to_string(),
        );
        app.chat.messages.push(sdk::ChatMessage::user_text("late"));

        let mismatches = app.dual_track_mismatches();

        assert!(mismatches
            .iter()
            .any(|item| item == "session.message_count"));
    }
}
