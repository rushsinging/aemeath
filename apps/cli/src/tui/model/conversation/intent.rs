#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationIntent {
    StartChat { submission: String },
    ObserveAssistantText { text: String },
    ObserveToolCallStart { name: String, index: usize },
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
        output: String,
        is_error: bool,
    },
    CompleteChat,
}
