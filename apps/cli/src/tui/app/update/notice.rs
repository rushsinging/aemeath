use crate::tui::app::App;
use crate::tui::model::conversation::intent::ConversationIntent;

impl App {
    /// 将一条系统提示消息写入单一真相源 `ConversationModel`，并刷新输出文档。
    ///
    /// 这是替代旧的命令式 `OutputArea::push_system` 的唯一入口：消息经
    /// `ConversationModel -> OutputViewModel -> OutputDocumentRenderer` 派生为
    /// `RenderedDocument`，不再直接写入 `OutputArea::lines`。
    pub(crate) fn append_system_notice(&mut self, text: impl Into<String>) {
        self.model
            .conversation
            .apply(ConversationIntent::AppendSystemMessage { text: text.into() });
        self.mark_output_dirty();
    }

    pub(crate) fn append_hook_notice(
        &mut self,
        content: crate::tui::model::conversation::block::HookNoticeContent,
    ) {
        self.model
            .conversation
            .apply(ConversationIntent::AppendHookNotice { content });
        self.mark_output_dirty();
    }

    /// 将一条用户输入回显写入单一真相源 `ConversationModel`，并刷新输出文档。
    ///
    /// 用于 ask_user 应答、队列输入冲刷等「在已激活回合内回显用户输入」的场景：
    /// 走 `AppendUserMessage` 而非 `StartChat`，不新开 chat、不破坏在途工具绑定。
    /// 回显经 `ConversationBlock::UserMessage -> UserMessage view -> "> ..."` 渲染。
    pub(crate) fn append_user_echo(&mut self, text: impl Into<String>) {
        self.model
            .conversation
            .apply(ConversationIntent::AppendUserMessage { text: text.into() });
        self.mark_output_dirty();
    }

    /// 将一条「排队中」用户提交写入单一真相源 `ConversationModel`，并刷新 live-status 投影。
    ///
    /// 用于「agent 处理期间用户提交输入」场景：在 `InputState::input_queue`
    /// 入队的同时，派发 `QueueSubmission` 写入 `ConversationModel::queued_submissions`。
    /// 该列表是排队预览的真相；`OutputArea::queued_submission_lines` 只是 live-status
    /// 渲染镜像，由 `refresh_live_status_from_model` 经 assembler/adapter 单向写回。
    ///
    /// 一致性约定：`input_queue` 为权威发送队列，`queued_submissions` 是其显示投影；
    /// 入队（此处）与出队（`clear_queued_submission_echo`）成对维护，二者始终同步。
    pub(crate) fn enqueue_submission_echo(&mut self, text: impl Into<String>) {
        self.model
            .conversation
            .apply(ConversationIntent::QueueSubmission { text: text.into() });
        self.mark_output_dirty();
        self.refresh_live_status_from_model();
    }

    /// 清除所有「排队中」用户提交，并刷新 live-status 投影。
    ///
    /// 在 agent 取用（drain）排队输入时调用：先清除 `queued_submissions`，
    /// 再由 `append_user_echo` 以正式 `UserMessage` 显示，避免「排队预览」与「已发送
    /// 回显」双显示。空队列时为无副作用 no-op（`QueuedSubmissionsCleared { count: 0 }`）。
    pub(crate) fn clear_queued_submission_echo(&mut self) {
        self.model
            .conversation
            .apply(ConversationIntent::ClearQueuedSubmissions);
        self.mark_output_dirty();
        self.refresh_live_status_from_model();
    }
    /// 将一条错误提示消息写入单一真相源 `ConversationModel`，并刷新输出文档。
    ///
    /// 替代旧的命令式 `OutputArea::push_error`；错误经 `ConversationBlock::Error`
    /// 映射为 `DiagnosticNotice`（Error 语义色）渲染。
    pub(crate) fn append_error_notice(&mut self, text: impl Into<String>) {
        self.model
            .conversation
            .apply(ConversationIntent::AppendError { text: text.into() });
        self.mark_output_dirty();
    }

    /// 显示 AskUserQuestion 交互块（问题 + 选项），作为渲染单一真相进入 ConversationModel。
    pub(crate) fn show_ask_user_block(
        &mut self,
        question: String,
        options: Vec<sdk::OptionItem>,
        llm_option_count: usize,
        multi_select: bool,
        cursor: usize,
        default: Option<String>,
    ) {
        self.model
            .conversation
            .apply(ConversationIntent::ShowAskUser {
                question,
                options,
                llm_option_count,
                multi_select,
                cursor,
                default,
            });
        self.mark_output_dirty();
    }

    /// 更新 AskUser 块光标位置（选项导航高亮），并刷新文档。
    pub(crate) fn set_ask_user_cursor(&mut self, cursor: usize) {
        self.model
            .conversation
            .apply(ConversationIntent::SetAskUserCursor { cursor });
        self.mark_output_dirty();
    }

    /// 切换 AskUser 块某选项勾选状态（multi_select），并刷新文档。
    pub(crate) fn toggle_ask_user_selected(&mut self, index: usize) {
        self.model
            .conversation
            .apply(ConversationIntent::ToggleAskUserSelected { index });
        self.mark_output_dirty();
    }

    /// 设置 AskUser 块是否处于「Chat about this...」自由输入子态，并刷新文档。
    pub(crate) fn set_ask_user_chat_input(&mut self, active: bool) {
        self.model
            .conversation
            .apply(ConversationIntent::SetAskUserChatInput { active });
        self.mark_output_dirty();
    }

    /// 移除 AskUser 交互块（用户提交/取消后折叠），并刷新文档。
    pub(crate) fn dismiss_ask_user_block(&mut self) {
        self.model
            .conversation
            .apply(ConversationIntent::DismissAskUser);
        self.mark_output_dirty();
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::app::App;
    use crate::tui::model::conversation::block::ConversationBlock;
    use std::path::PathBuf;

    fn make_app() -> App {
        App::new(
            "sess-notice".to_string(),
            PathBuf::from("/tmp"),
            "test-model".to_string(),
        )
    }

    #[test]
    fn test_append_system_notice_pushes_system_block() {
        let mut app = make_app();
        app.append_system_notice("你好");
        let has_system =
            app.model.conversation.blocks.iter().any(
                |block| matches!(block, ConversationBlock::System { text, .. } if text == "你好"),
            );
        assert!(
            has_system,
            "系统消息应作为 System block 进入 ConversationModel"
        );
    }

    #[test]
    fn test_append_error_notice_pushes_error_block() {
        let mut app = make_app();
        app.append_error_notice("出错了");
        let has_error = app.model.conversation.blocks.iter().any(
            |block| matches!(block, ConversationBlock::Error { text, .. } if text == "出错了"),
        );
        assert!(
            has_error,
            "错误消息应作为 Error block 进入 ConversationModel"
        );
    }

    #[test]
    fn test_append_system_notice_renders_into_document() {
        let mut app = make_app();
        // 边界：banner 由 init() 写入 legacy lines，document 此时为空。
        // 派发系统消息后，document 必须经 ViewModel 派生出非空 block。
        app.append_system_notice("渲染检查");
        app.flush_dirty_view_models();
        let plain = app
            .output_area
            .document()
            .iter_lines()
            .map(|line| line.plain.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            plain.contains("渲染检查"),
            "系统消息应经 document 渲染出现在输出区，实际: {plain:?}"
        );
    }

    #[test]
    fn test_append_user_echo_pushes_user_block_without_new_chat() {
        let mut app = make_app();
        app.model.conversation.apply(
            crate::tui::model::conversation::intent::ConversationIntent::StartChat {
                submission: "原始提问".to_string(),
            },
        );
        let chats_before = app.model.conversation.chats.len();

        app.append_user_echo("我的答复");

        // 正常路径：回显作为 UserMessage 块进入模型，但不新开 chat。
        assert_eq!(
            app.model.conversation.chats.len(),
            chats_before,
            "回显不应新建 chat"
        );
        let has_user = app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::UserMessage { text, .. } if text == "我的答复")
        });
        assert!(
            has_user,
            "回显应作为 UserMessage block 进入 ConversationModel"
        );
    }

    #[test]
    fn test_append_user_echo_renders_gt_prefix_into_document() {
        let mut app = make_app();
        app.append_user_echo("回显检查");
        app.flush_dirty_view_models();
        // `> ` marker 现由 gutter 注入到行首 span（plain 仅含内容）；断言渲染文档中
        // 存在「行首 gutter span == `> ` 且内容为回显文本」的行，验证回显仍带 `> ` 前缀。
        let has_echo = app.output_area.document().iter_lines().any(|line| {
            line.plain == "回显检查"
                && line
                    .spans
                    .first()
                    .is_some_and(|s| s.content.as_ref() == "> ")
        });
        assert!(
            has_echo,
            "用户回显应以 gutter 注入的 \"> \" 前缀 span 渲染（plain 为内容原文）"
        );
    }

    #[test]
    fn test_append_user_echo_empty_text_still_creates_block() {
        let mut app = make_app();
        let before = app.model.conversation.blocks.len();
        app.append_user_echo("");
        assert_eq!(
            app.model.conversation.blocks.len(),
            before + 1,
            "空回显文本仍应创建一个 UserMessage block"
        );
    }

    #[test]
    fn test_append_error_notice_empty_text_still_creates_block() {
        let mut app = make_app();
        let before = app.model.conversation.blocks.len();
        app.append_error_notice("");
        assert_eq!(
            app.model.conversation.blocks.len(),
            before + 1,
            "空错误文本仍应创建一个 Error block"
        );
    }

    #[test]
    fn test_enqueue_submission_echo_renders_queued_block_into_model() {
        // 正常路径：入队即时显示——派发后 QueuedUserMessage 块进入模型。
        // 渲染不再经 document block，改为 live-status projection。
        let mut app = make_app();
        app.enqueue_submission_echo("排队中的输入");

        let has_queued = app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::QueuedUserMessage { text, .. } if text == "排队中的输入")
        });
        assert!(
            has_queued,
            "入队应作为 QueuedUserMessage block 进入 ConversationModel"
        );

        // queued_submission 不再出现在 document 中（已移至 live-status projection）。
        let plain = app
            .output_area
            .document()
            .iter_lines()
            .map(|line| line.plain.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !plain.contains(">"),
            "排队提交不应出现在 document 渲染中，实际: {plain:?}"
        );
    }

    #[test]
    fn test_enqueue_submission_echo_refreshes_live_status_projection() {
        let mut app = make_app();
        app.enqueue_submission_echo("排队中的输入");

        assert_eq!(
            app.live_status_view_model().queued_lines,
            vec!["> 排队中的输入"],
            "入队后应可从 live-status projection 派生排队输入"
        );
    }

    #[test]
    fn test_enqueue_submission_echo_uses_display_text_for_copied_text() {
        let mut app = make_app();
        app.enqueue_submission_echo("[Copied Text 1]");

        let has_queued = app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::QueuedUserMessage { text, .. } if text == "[Copied Text 1]")
        });
        assert!(has_queued, "排队区应显示折叠占位符");
        assert_eq!(
            app.live_status_view_model().queued_lines,
            vec!["> [Copied Text 1]"]
        );
    }

    #[test]
    fn test_clear_queued_submission_echo_removes_queued_blocks_no_double_display() {
        // 边界 + 关键：drain 时先清排队块，再以正式 UserMessage 回显——
        // 验证清除后不再有 QueuedUserMessage（避免与已发送回显双显示）。
        let mut app = make_app();
        app.enqueue_submission_echo("第一条");
        app.enqueue_submission_echo("第二条");
        assert_eq!(app.model.conversation.queued_submissions.len(), 2);

        app.clear_queued_submission_echo();
        // 模拟 drain 后正式回显其中一条。
        app.append_user_echo("第一条");

        assert!(app.model.conversation.queued_submissions.is_empty());
        let queued_remaining = app
            .model
            .conversation
            .blocks
            .iter()
            .filter(|block| matches!(block, ConversationBlock::QueuedUserMessage { .. }))
            .count();
        assert_eq!(
            queued_remaining, 0,
            "清除后不应残留任何 QueuedUserMessage 排队块"
        );
        let user_echoes = app
            .model
            .conversation
            .blocks
            .iter()
            .filter(|block| {
                matches!(block, ConversationBlock::UserMessage { text, .. } if text == "第一条")
            })
            .count();
        assert_eq!(user_echoes, 1, "应仅以一条正式 UserMessage 回显，无双显示");
    }

    #[test]
    fn test_clear_queued_submission_echo_on_empty_is_noop() {
        // 错误/空路径：无排队块时清除应为无副作用 no-op，不 panic、不改变块数量。
        let mut app = make_app();
        let before = app.model.conversation.blocks.len();
        app.clear_queued_submission_echo();
        assert_eq!(
            app.model.conversation.blocks.len(),
            before,
            "空队列清除不应改变块数量"
        );
        assert!(app.model.conversation.queued_submissions.is_empty());
    }

    #[test]
    fn test_assistant_after_system_notice_uses_assistant_color() {
        // #74 回归端到端测试：System block（Muted 暗色）后追加 AssistantText block，
        // 验证 document 渲染中 assistant 行使用 ASSISTANT 色而非继承 System 的 Muted 色。
        use crate::tui::model::conversation::intent::ConversationIntent;
        use crate::tui::render::theme;

        let mut app = make_app();
        // 模拟 reflection 输出（System block）
        app.append_system_notice("reflection 输出内容");
        // 模拟后续 LLM 回复（Assistant block）
        app.model
            .conversation
            .apply(ConversationIntent::ObserveAssistantText {
                chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
                text: "后续回复".to_string(),
            });
        app.refresh_output_document_from_model();

        // 在 document 中找到包含"后续回复"的行
        let assistant_line = app
            .output_area
            .document()
            .iter_lines()
            .find(|line| line.plain.contains("后续回复"))
            .expect("应渲染 assistant 文本");

        let fg = assistant_line
            .spans
            .iter()
            .find(|s| s.content.as_ref().contains("后续回复"))
            .map(|s| s.style.fg)
            .expect("应找到 assistant span");

        assert_eq!(
            fg,
            Some(theme::ASSISTANT),
            "System block 后的 Assistant block 应使用 ASSISTANT 色 ({:?})，而非 Muted ({:?})",
            theme::ASSISTANT,
            theme::TEXT_MUTED
        );
    }

    #[test]
    fn test_streaming_assistant_interrupted_by_system_uses_assistant_color() {
        // #74 场景：streaming assistant text 被 System notice 中断后，
        // 后续 streaming text 仍应使用 ASSISTANT 色。
        use crate::tui::model::conversation::intent::ConversationIntent;
        use crate::tui::render::theme;

        let mut app = make_app();
        // 模拟用户提问
        app.model.conversation.apply(ConversationIntent::StartChat {
            submission: "hello".to_string(),
        });
        // 模拟 LLM streaming
        app.model
            .conversation
            .apply(ConversationIntent::ObserveAssistantText {
                chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
                text: "你好".to_string(),
            });
        app.refresh_output_document_from_model();
        // 模拟 System notice 中断（如自动 reflection）
        app.append_system_notice("[reflection: ...]");
        app.flush_dirty_view_models();
        // 模拟 LLM streaming 继续
        app.model
            .conversation
            .apply(ConversationIntent::ObserveAssistantText {
                chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
                text: "世界".to_string(),
            });
        app.refresh_output_document_from_model();

        // 验证"你好"和"世界"都在 document 中且使用 ASSISTANT 色
        for needle in &["你好", "世界"] {
            let line = app
                .output_area
                .document()
                .iter_lines()
                .find(|line| line.plain.contains(needle))
                .unwrap_or_else(|| panic!("应渲染文本: {needle}"));
            let fg = line
                .spans
                .iter()
                .find(|s| s.content.as_ref().contains(needle))
                .map(|s| s.style.fg)
                .unwrap_or_else(|| panic!("应找到 span: {needle}"));
            assert_eq!(
                fg,
                Some(theme::ASSISTANT),
                "\"{needle}\" 应使用 ASSISTANT 色，实际: {fg:?}"
            );
        }
    }
}
