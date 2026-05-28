#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationIntent {
    StartChat {
        submission: String,
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
