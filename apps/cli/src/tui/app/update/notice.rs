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
        self.refresh_output_widget_from_model();
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
        self.refresh_output_widget_from_model();
    }

    /// 将一条错误提示消息写入单一真相源 `ConversationModel`，并刷新输出文档。
    ///
    /// 替代旧的命令式 `OutputArea::push_error`；错误经 `ConversationBlock::Error`
    /// 映射为 `DiagnosticNotice`（Error 语义色）渲染。
    pub(crate) fn append_error_notice(&mut self, text: impl Into<String>) {
        self.model
            .conversation
            .apply(ConversationIntent::AppendError { text: text.into() });
        self.refresh_output_widget_from_model();
    }

    /// 显示 AskUserQuestion 交互块（问题 + 选项），作为渲染单一真相进入 ConversationModel。
    pub(crate) fn show_ask_user_block(
        &mut self,
        question: String,
        options: Vec<String>,
        llm_option_count: usize,
        multi_select: bool,
        cursor: usize,
    ) {
        self.model
            .conversation
            .apply(ConversationIntent::ShowAskUser {
                question,
                options,
                llm_option_count,
                multi_select,
                cursor,
            });
        self.refresh_output_widget_from_model();
    }

    /// 更新 AskUser 块光标位置（选项导航高亮），并刷新文档。
    pub(crate) fn set_ask_user_cursor(&mut self, cursor: usize) {
        self.model
            .conversation
            .apply(ConversationIntent::SetAskUserCursor { cursor });
        self.refresh_output_widget_from_model();
    }

    /// 切换 AskUser 块某选项勾选状态（multi_select），并刷新文档。
    pub(crate) fn toggle_ask_user_selected(&mut self, index: usize) {
        self.model
            .conversation
            .apply(ConversationIntent::ToggleAskUserSelected { index });
        self.refresh_output_widget_from_model();
    }

    /// 设置 AskUser 块是否处于「Chat about this...」自由输入子态，并刷新文档。
    pub(crate) fn set_ask_user_chat_input(&mut self, active: bool) {
        self.model
            .conversation
            .apply(ConversationIntent::SetAskUserChatInput { active });
        self.refresh_output_widget_from_model();
    }

    /// 移除 AskUser 交互块（用户提交/取消后折叠），并刷新文档。
    pub(crate) fn dismiss_ask_user_block(&mut self) {
        self.model
            .conversation
            .apply(ConversationIntent::DismissAskUser);
        self.refresh_output_widget_from_model();
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
        let plain = app
            .output_area
            .document()
            .iter_lines()
            .map(|line| line.plain.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            plain.contains("> 回显检查"),
            "用户回显应以 \"> \" 前缀经 document 渲染，实际: {plain:?}"
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
}
