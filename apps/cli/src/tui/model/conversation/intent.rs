#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationIntent {
    StartChat {
        submission: String,
    },
    /// 仅追加一条用户消息回显块，不创建新的 chat/turn。
    ///
    /// 用于 ask_user 应答、队列输入冲刷等「在已激活的对话回合内回显用户输入」的场景，
    /// 此时不能用 `StartChat`（会新开 chat 并重置 active_chat_id，破坏在途工具绑定）。
    AppendUserMessage {
        text: String,
    },
    ObserveAssistantText {
        text: String,
    },
    ObserveThinkingText {
        text: String,
    },
    CompleteTextBlock,
    ObserveToolCallStart {
        name: String,
        index: usize,
    },
    ObserveToolArguments {
        name: String,
        index: usize,
        partial_args: String,
    },
    ObserveToolCall {
        id: String,
        name: String,
        index: usize,
        summary: String,
    },
    ObserveToolResult {
        id: String,
        tool_name: String,
        output: String,
        is_error: bool,
        image_count: usize,
    },
    AppendSystemMessage {
        text: String,
    },
    AppendError {
        text: String,
    },
    QueueSubmission {
        text: String,
    },
    ClearQueuedSubmissions,
    RecordAgentProgress {
        tool_id: String,
        message: String,
    },
    CompleteChat,
}
