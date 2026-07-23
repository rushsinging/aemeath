use crate::tui::app::App;
use crate::tui::model::conversation::intent::*;
use crate::tui::update::intent::AgentIntent;

impl App {
    /// 将一条系统提示消息写入单一真相源 `ConversationModel`，并刷新输出文档。
    ///
    /// 这是替代旧的命令式 `OutputArea::push_system` 的唯一入口：消息经
    /// `ConversationModel -> OutputViewModel -> OutputDocumentRenderer` 派生为
    /// `RenderedDocument`，不再直接写入 `OutputArea::lines`。
    pub(crate) fn append_system_notice(&mut self, text: impl Into<String>) {
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::AppendSystemMessage(AppendSystemMessage { text: text.into() }),
        ));
    }

    /// 将一条用户输入回显写入单一真相源 `ConversationModel`，并刷新输出文档。
    ///
    /// 用于 ask_user 应答、队列输入冲刷等「在已激活回合内回显用户输入」的场景：
    /// 走 `AppendUserMessage` 而非 `StartChat`，不新开 chat、不破坏在途工具绑定。
    /// 回显经 `ConversationBlock::UserMessage -> UserMessage view -> "> ..."` 渲染。
    pub(crate) fn append_user_echo(&mut self, text: impl Into<String>) {
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::AppendUserMessage(AppendUserMessage { text: text.into() }),
        ));
    }

    /// 将一条「排队中」用户提交写入单一真相源 `ConversationModel`，并刷新 live-status 投影。
    ///
    /// 用于「agent 处理期间用户提交输入」场景：派发 `QueueSubmission` 写入
    /// `ConversationModel::queued_submissions`（排队预览的真相）；`OutputArea::queued_submission_lines`
    /// 只是 live-status 渲染镜像，由 `refresh_live_status_from_model` 经 assembler/adapter 单向写回。
    pub(crate) fn enqueue_submission_echo(
        &mut self,
        input_id: impl AsRef<str>,
        text: impl Into<String>,
    ) {
        let input_id = input_id.as_ref();
        let text_str = text.into();
        let text_len = text_str.chars().count();
        let before_count = self.model.conversation.queued_submissions.len();
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::QueueSubmission(QueueSubmission {
                input_id: input_id.to_string(),
                text: text_str,
            }),
        ));
        let after_count = self.model.conversation.queued_submissions.len();
        self.mark_output_dirty();
        self.refresh_live_status_from_model();
        crate::tui::log_debug!(
            "enqueue_submission_echo input_id={} text_len={} queued_count {}->{} output_dirty={}",
            input_id,
            text_len,
            before_count,
            after_count,
            self.view_state.dirty.output
        );
    }

    /// 按 InputId 精确清除单条「排队中」用户提交，并刷新 live-status 投影。
    ///
    /// 用于 UserMessagesAdopted handler 按 input_id 逐条清除占位的场景：仅移除与
    /// 给定 `input_id` 匹配的那一条，不影响其他排队项。
    /// （A3：原「全清」版本已随文本队列废弃一并删除，回显只按 id 精确清除。）
    pub(crate) fn clear_queued_submission_echo_by_id(&mut self, input_id: &str) {
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::ClearQueuedSubmissionById(ClearQueuedSubmissionById {
                input_id: input_id.to_string(),
            }),
        ));
        self.refresh_live_status_from_model();
    }

    /// 将一条错误提示消息写入单一真相源 `ConversationModel`，并刷新输出文档。
    ///
    /// 替代旧的命令式 `OutputArea::push_error`；错误经 `ConversationBlock::Error`
    /// 映射为 `DiagnosticNotice`（Error 语义色）渲染。
    pub(crate) fn append_error_notice(&mut self, text: impl Into<String>) {
        self.apply_agent_intent(AgentIntent::Conversation(ConversationIntent::AppendError(
            AppendError { text: text.into() },
        )));
    }
}

#[cfg(test)]
#[path = "notice_tests.rs"]
mod notice_tests;
