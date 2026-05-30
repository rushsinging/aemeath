#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests {
    use crate::tui::app::state::{ChatState, InputState, SessionState, UiLayout};
    use crate::tui::app::{App, UiEvent};
    use crate::tui::effect::session::processing::SpawnContextRefs;
    use crate::tui::update::msg::TuiMsg;

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
            cached_models: vec![],
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
            cached_models: vec![],
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
            cached_models: vec![],
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
            cached_models: vec![],
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
    fn test_app_model_initializes_session_runtime_and_input() {
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
    }

    #[test]
    fn test_app_model_starts_with_empty_usage_and_attachments() {
        let app = App::new(
            "sess-usage".to_string(),
            std::path::PathBuf::from("/tmp/aemeath"),
            "gpt-test".to_string(),
        );

        assert_eq!(app.model.session.message_count, 0);
        assert_eq!(app.model.runtime.usage.input_tokens, 0);
        assert_eq!(app.model.runtime.usage.output_tokens, 0);
        assert!(app.model.input.attachments.is_empty());
    }

    #[test]
    fn test_app_model_is_independent_from_removed_legacy_facade() {
        let mut app = App::new(
            "sess-independent".to_string(),
            std::path::PathBuf::from("/tmp/aemeath"),
            "gpt-test".to_string(),
        );
        app.chat.messages.push(sdk::ChatMessage::user_text("late"));

        assert_eq!(app.model.session.message_count, 0);
    }

    #[tokio::test]
    async fn test_update_enter_starts_model_conversation_for_tool_rendering() {
        let mut app = App::new(
            "sess-e2e".to_string(),
            std::path::PathBuf::from("/tmp/aemeath"),
            "gpt-test".to_string(),
        );
        app.model
            .input
            .apply(crate::tui::model::input::intent::InputIntent::InsertText(
                "search bug 76".to_string(),
            ));
        let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel(8);
        let spawn_refs = SpawnContextRefs { agent_client: None };

        let _ = app.update(TuiMsg::Key(enter_key()), &ui_tx, &spawn_refs);

        assert!(app.model.conversation.active_chat_id.is_some());
    }

    #[tokio::test]
    async fn test_thinking_then_grep_renders_tool_block_in_output_area() {
        let mut app = App::new(
            "sess-grep".to_string(),
            std::path::PathBuf::from("/tmp/aemeath"),
            "gpt-test".to_string(),
        );
        app.model
            .input
            .apply(crate::tui::model::input::intent::InputIntent::InsertText(
                "search bug 76".to_string(),
            ));
        app.output_area.last_visible_height = 3;
        // 滚动真相归 view_state；设 stale offset，渲染前由 adapter 钳制并写回 widget。
        app.view_state.output.scroll_offset = 99;
        // 设置真实宽度，避免输出渲染按 width=1 逐字换行（G2 起工具结果走宽度换行）。
        app.layout.output_area_rect = ratatui::layout::Rect::new(0, 0, 100, 40);
        let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel(8);
        let spawn_refs = SpawnContextRefs { agent_client: None };

        let _ = app.update(TuiMsg::Key(enter_key()), &ui_tx, &spawn_refs);
        for event in grep_after_thinking_events() {
            let _ = app.update(TuiMsg::Ui(event), &ui_tx, &spawn_refs);
        }

        // marker（>/✓）与块级缩进现由 gutter 注入到行首 span（plain 保持内容原文）；
        // 故拼接 span 内容复现 gutter 后的可见行文本进行断言。
        let rendered = render_output_rows(&app.output_area);

        // gutter 为每个 block 注入 2 列行首槽：UserMessage→`> `，ToolCall→状态字形 + 空格，
        // 其余 block→2 空格。故所有可见行均带 gutter 前缀。
        // 启动横幅现纳入 ConversationModel，用户消息不再是首行。
        assert!(rendered.iter().any(|line| line == "> search bug 76"));
        assert!(rendered.iter().any(|line| line == "  Aemeath - AI Agent"));
        assert!(rendered.iter().any(|line| line == "  💭 thinking"));
        assert!(rendered.iter().any(|line| line == "✓ Grep /76/"));
        // 工具 detail/result 行 gutter 给等宽空白（2 列），与旧 INDENT 视觉一致。
        assert!(rendered
            .iter()
            .any(|line| line == "  in docs/bug/active.md"));
        // 结果升为 depth-1 子块（#60）：gutter = 2(深度缩进) + 2(空白 marker 槽) = 4 列前导。
        // result 子块展示工具 output 前 N 行预览（Grep result_max_lines=5；6 行 output →
        // 前 5 行 + "1 lines omitted"），不再退化为纯 "✓ Grep completed" 摘要。
        assert!(rendered
            .iter()
            .any(|line| line == "    /tmp/docs/bug/active.md:18:match"));
        assert!(rendered
            .iter()
            .any(|line| line == "    ... (1 lines omitted)"));
        assert!(!rendered.iter().any(|line| line.contains("Grep completed")));
        assert!(!rendered.iter().any(|line| line.contains("You:")));
        assert!(!rendered
            .iter()
            .any(|line| line == "/tmp/docs/bug/active.md:18:match"));
        // 渲染前滚动写回：view_state stale offset 经 adapter 钳制后镜像到 widget。
        app.refresh_output_scroll_from_view_state();
        assert!(app.output_area.scroll_offset <= app.output_area.document().total_lines());
    }

    /// 拼接 document 各行的 span 内容（gutter span + 内容 span）为逻辑可见行。
    /// gutter（marker/缩进）注入到行首 span 而非 plain，故拼接 span 才能复现屏幕文本，
    /// 且避免 buffer cell 回读对 CJK 宽字符的拆分。`output_area` 取引用即可（document 已就绪）。
    fn render_output_rows(
        output_area: &crate::tui::render::output_area::OutputArea,
    ) -> Vec<String> {
        output_area
            .document()
            .iter_lines()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    fn enter_key() -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent {
            code: crossterm::event::KeyCode::Enter,
            modifiers: crossterm::event::KeyModifiers::NONE,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn grep_after_thinking_events() -> Vec<UiEvent> {
        vec![
            UiEvent::Thinking("thinking".to_string()),
            UiEvent::TextBlockComplete("thinking".to_string()),
            UiEvent::ToolCallStart {
                name: "Grep".to_string(),
                index: 1,
            },
            UiEvent::ToolCall {
                id: "grep-1".to_string(),
                name: "Grep".to_string(),
                index: Some(1),
                summary: r#"{"pattern":"76","path":"docs/bug/active.md"}"#.to_string(),
            },
            UiEvent::ToolResult {
                id: "grep-1".to_string(),
                tool_name: "Grep".to_string(),
                output: "/tmp/docs/bug/active.md:18:match\n/tmp/docs/bug/active.md:19:next\n/tmp/docs/bug/active.md:20:more\n/tmp/docs/bug/active.md:21:more\n/tmp/docs/bug/active.md:22:more\n/tmp/docs/bug/active.md:23:omitted".to_string(),
                is_error: false,
                images: Vec::new(),
            },
        ]
    }
}
