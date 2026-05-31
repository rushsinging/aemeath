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
    /// 显示 AskUserQuestion 交互块（问题 + 选项），替换已有的 AskUser 块（若有）。
    ShowAskUser {
        question: String,
        options: Vec<sdk::OptionItem>,
        llm_option_count: usize,
        multi_select: bool,
        cursor: usize,
        /// 无选项自由输入模式下的默认值提示。
        default: Option<String>,
    },
    /// 更新 AskUser 块的光标位置（选项导航高亮的单一真相）。
    SetAskUserCursor {
        cursor: usize,
    },
    /// 切换 AskUser 块中某选项的勾选状态（multi_select）。
    ToggleAskUserSelected {
        index: usize,
    },
    /// 设置 AskUser 块是否处于「Chat about this...」自由输入子态。
    SetAskUserChatInput {
        active: bool,
    },
    /// 追加字符到 Type something 输入框。
    AppendAskUserChatChar {
        ch: char,
    },
    /// 删除 Type something 输入框末尾字符。
    DeleteAskUserChatChar,
    /// 移除 AskUser 交互块（用户提交答案或取消后折叠）。
    DismissAskUser,
    CompleteChat,
}
