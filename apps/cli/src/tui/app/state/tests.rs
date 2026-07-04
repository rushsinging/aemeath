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
    fn test_input_state_default() {
        let state = InputState::default();
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
        assert_eq!(
            app.model.conversation.runtime.model_id.as_deref(),
            Some("gpt-test")
        );
        assert_eq!(
            app.model.conversation.runtime.workspace.cwd,
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
        assert_eq!(app.model.conversation.runtime.usage.input_tokens, 0);
        assert_eq!(app.model.conversation.runtime.usage.output_tokens, 0);
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
    async fn test_submit_then_user_messages_added_echoes_user_message() {
        // A3 Task 4：MessagesSync 退出 display，用户回显改由 UserMessagesAdopted 驱动。
        // 模拟流程：Enter 提交 → runtime 回传 UserMessagesAdopted → TUI 回显 `> search bug 76`；
        // MessagesSync 仅负责镜像 chat.messages，不再产生 UserMessage 回显块。
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

        // 提交：不再立即 StartChat，仅产生 SendChatInputEvent。
        let result = app.update(TuiMsg::Key(enter_key()), &ui_tx, &spawn_refs);
        assert!(
            result.effects.iter().any(|effect| matches!(
                effect,
                crate::tui::effect::effect::Effect::SendChatInputEvent {
                    event: sdk::ChatInputEvent::UserMessage { text, .. }
                } if text == "search bug 76"
            )),
            "首条提交应经事件通道发 UserMessage"
        );

        // 模拟 runtime 回传 UserMessagesAdopted（归宿事件，携带 InputId，驱动 TUI 回显）。
        let input_id = sdk::InputId::new_v7();
        app.enqueue_submission_echo(input_id.clone(), "search bug 76");
        let _ = app.update(
            TuiMsg::Ui(UiEvent::UserMessagesAdopted {
                items: vec![sdk::ChatMessage {
                    role: "user".to_string(),
                    content: vec![sdk::ContentBlock::text("search bug 76")],
                    metadata: None,
                    input_id: Some(input_id),
                }],
                queued: vec![],
            }),
            &ui_tx,
            &spawn_refs,
        );

        let has_user_echo = app.model.conversation.timeline.items().iter().any(|item| {
            matches!(
                item,
                crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. }
                    if text == "search bug 76"
            )
        });
        assert!(
            has_user_echo,
            "UserMessagesAdopted 应回显用户消息为 UserMessage 块"
        );

        // 同步 MessagesSync 仅镜像，不额外产生回显块
        let echo_count_before = app
            .model
            .conversation
            .timeline
            .items()
            .iter()
            .filter(|b| {
                matches!(b, crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. } if text == "search bug 76")
            })
            .count();
        let _ = app.update(
            TuiMsg::Ui(UiEvent::TurnStarted {
                messages: vec![sdk::ChatMessage::user_text("search bug 76")],
            }),
            &ui_tx,
            &spawn_refs,
        );
        let echo_count_after = app
            .model
            .conversation
            .timeline
            .items()
            .iter()
            .filter(|b| {
                matches!(b, crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. } if text == "search bug 76")
            })
            .count();
        assert_eq!(
            echo_count_before, echo_count_after,
            "MessagesSync 不应再增加 UserMessage 回显块（已退出 display）"
        );
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
        app.view_state.output.last_visible_height = 3;
        // 滚动真相归 view_state；设 stale offset，渲染前由 view_state document metrics 钳制。
        app.view_state.output.scroll_offset = 99;
        // 设置真实宽度，避免输出渲染按 width=1 逐字换行（G2 起工具结果走宽度换行）。
        app.layout.output_area_rect = ratatui::layout::Rect::new(0, 0, 100, 40);
        let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel(8);
        let spawn_refs = SpawnContextRefs { agent_client: None };

        let _ = app.update(TuiMsg::Key(enter_key()), &ui_tx, &spawn_refs);
        // A3 Task 4：用户回显改由 UserMessagesAdopted 归宿事件驱动（MessagesSync 已退出 display）。
        // 先建占位，再触发 UserMessagesAdopted，使 `> search bug 76` 回显。
        let input_id = sdk::InputId::new_v7();
        app.enqueue_submission_echo(input_id.clone(), "search bug 76");
        let _ = app.update(
            TuiMsg::Ui(UiEvent::UserMessagesAdopted {
                items: vec![sdk::ChatMessage {
                    role: "user".to_string(),
                    content: vec![sdk::ContentBlock::text("search bug 76")],
                    metadata: None,
                    input_id: Some(input_id),
                }],
                queued: vec![],
            }),
            &ui_tx,
            &spawn_refs,
        );
        for event in grep_after_thinking_events() {
            let _ = app.update(TuiMsg::Ui(event), &ui_tx, &spawn_refs);
        }
        app.flush_dirty_view_models();

        // marker（>/✓）与块级缩进现由 gutter 注入到行首 span（plain 保持内容原文）；
        // 故拼接 span 内容复现 gutter 后的可见行文本进行断言。
        let rendered = render_output_rows(&app.output_area);

        // gutter 为每个 block 注入行首 marker 槽：UserMessage→`> `，ToolCall→状态字形 + 空格，
        // ThinkingMessage→`💭`（宽字符占满 2 列槽，顶格、内容与其它 block 同列对齐），
        // 其余 block→2 空格。启动横幅现纳入 ConversationModel，用户消息不再是首行。
        assert!(rendered.iter().any(|line| line == "> search bug 76"));
        assert!(rendered.iter().any(|line| line == "  Aemeath - AI Agent"));
        assert!(rendered.iter().any(|line| line == "💭thinking"));
        // Grep header 现在包含 pattern 和 path
        assert!(rendered
            .iter()
            .any(|line| line.contains("Search /76/") && line.contains("docs/bug/active.md")));
        // Grep details 已隐藏（path 已在 header 中）
        // 结果升为 depth-1 子块（#60）：gutter = 2(深度缩进) + 2(marker 槽) = 4 列前导。
        // 首行 marker 为 ⎿ 圆角连接（连接父 ToolCall），续行为 4 空格。
        // result 子块展示工具 output 前 N 行预览（Grep result_max_lines=5；6 行 output →
        // 前 5 行 + "1 lines omitted"），不再退化为纯 "✓ Grep completed" 摘要。
        assert!(rendered
            .iter()
            .any(|line| line == "  ⎿ /tmp/docs/bug/active.md:18:match"));
        assert!(rendered
            .iter()
            .any(|line| line == "    ... (1 lines omitted)"));
        assert!(!rendered
            .iter()
            .any(|line| line.contains("Search completed")));
        assert!(!rendered.iter().any(|line| line.contains("You:")));
        assert!(!rendered
            .iter()
            .any(|line| line == "/tmp/docs/bug/active.md:18:match"));
        // 渲染前滚动同步：view_state stale offset 经 adapter 钳制。
        app.refresh_output_scroll_from_view_state();
        assert!(app.view_state.output.scroll_offset <= app.output_area.document().total_lines());
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

    fn test_turn_context() -> crate::tui::app::event::UiTurnContext {
        crate::tui::app::event::UiTurnContext {
            chat_id: crate::tui::model::conversation::ids::ChatId::new("chat-test"),
            turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-test"),
        }
    }

    fn grep_after_thinking_events() -> Vec<UiEvent> {
        vec![
            UiEvent::Thinking {
                context: test_turn_context(),
                text: "thinking".to_string(),
            },
            UiEvent::BlockComplete {
                context: test_turn_context(),
                text: "thinking".to_string(),
            },
            UiEvent::ToolCallStart {
                context: test_turn_context(),
                id: sdk::ids::ToolCallId::new("grep-1"),                provider_id: Some("provider-grep-1".to_string()),
                name: "Grep".to_string(),
                index: 1,
            },
            UiEvent::ToolCallUpdate {
                context: test_turn_context(),
                id: sdk::ids::ToolCallId::new("grep-1"),
                provider_id: Some("provider-grep-1".to_string()),
                name: "Grep".to_string(),
                index: 1,
                arguments_delta: None,
                arguments: Some(serde_json::json!({
                    "pattern": "76",
                    "path": "docs/bug/active.md"
                })),
                status: sdk::ToolCallStatusView::Ready,
            },
            UiEvent::ToolResult {
                context: test_turn_context(),
                id: sdk::ids::ToolCallId::new("grep-1"),
                provider_id: "provider-grep-1".to_string(),
                tool_name: "Grep".to_string(),
                output: "/tmp/docs/bug/active.md:18:match\n/tmp/docs/bug/active.md:19:next\n/tmp/docs/bug/active.md:20:more\n/tmp/docs/bug/active.md:21:more\n/tmp/docs/bug/active.md:22:more\n/tmp/docs/bug/active.md:23:omitted".to_string(),
                content: serde_json::json!({ "text": "/tmp/docs/bug/active.md:18:match\n/tmp/docs/bug/active.md:19:next\n/tmp/docs/bug/active.md:20:more\n/tmp/docs/bug/active.md:21:more\n/tmp/docs/bug/active.md:22:more\n/tmp/docs/bug/active.md:23:omitted" }),
                is_error: false,
                images: Vec::new(),
            },
        ]
    }
}
