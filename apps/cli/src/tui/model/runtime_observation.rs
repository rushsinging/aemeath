use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::tool_call::ToolCallStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeTurnContext {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}

impl RuntimeTurnContext {
    pub fn new(chat_id: ChatId, turn_id: ChatTurnId) -> Self {
        Self { chat_id, turn_id }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeObservation {
    AssistantText {
        context: RuntimeTurnContext,
        text: String,
    },
    ThinkingText {
        context: RuntimeTurnContext,
        text: String,
    },
    BlockComplete {
        context: RuntimeTurnContext,
    },
    ToolCallStart {
        context: RuntimeTurnContext,
        id: ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ToolCallUpdate {
        context: RuntimeTurnContext,
        id: ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments: Option<String>,
        summary: Option<String>,
        status: ToolCallStatus,
    },
    ToolResult {
        context: RuntimeTurnContext,
        id: ToolCallId,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        image_count: usize,
    },
    AgentProgress {
        context: RuntimeTurnContext,
        tool_id: ToolCallId,
        message: String,
    },
    Complete {
        context: RuntimeTurnContext,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_observation_exposes_explicit_context() {
        let context = RuntimeTurnContext::new(ChatId::new_v7(), ChatTurnId::new_v7());
        let observation = RuntimeObservation::AssistantText {
            context: context.clone(),
            text: "hello".to_string(),
        };

        match observation {
            RuntimeObservation::AssistantText { context, text } => {
                assert_eq!(context.chat_id, context.chat_id);
                assert_eq!(context.turn_id, context.turn_id);
                assert_eq!(text, "hello");
            }
            other => panic!("unexpected observation: {other:?}"),
        }
    }
}
