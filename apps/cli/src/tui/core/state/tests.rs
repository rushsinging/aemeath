#[cfg(test)]
mod tests {
    use crate::tui::core::state::{ChatState, InputState, SessionState, UiLayout};

    fn make_memory_config() -> sdk::MemoryConfigView {
        sdk::MemoryConfigView::default()
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
        assert!(state.cached_sessions.is_empty());
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
}
