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
        let has_system = app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::System { text, .. } if text == "你好")
        });
        assert!(has_system, "系统消息应作为 System block 进入 ConversationModel");
    }

    #[test]
    fn test_append_error_notice_pushes_error_block() {
        let mut app = make_app();
        app.append_error_notice("出错了");
        let has_error = app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::Error { text, .. } if text == "出错了")
        });
        assert!(has_error, "错误消息应作为 Error block 进入 ConversationModel");
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
