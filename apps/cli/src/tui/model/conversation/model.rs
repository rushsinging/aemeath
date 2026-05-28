use super::change::ConversationChange;
use super::chat::{Chat, ChatStatus};
use super::ids::{ChatId, ToolCallId};
use super::intent::ConversationIntent;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConversationModel {
    pub chats: Vec<Chat>,
    pub active_chat_id: Option<ChatId>,
    next_chat_sequence: usize,
}

impl ConversationModel {
    pub fn apply(&mut self, intent: ConversationIntent) -> Vec<ConversationChange> {
        match intent {
            ConversationIntent::StartChat { submission } => self.start_chat(submission),
            ConversationIntent::ObserveToolCallStart { name, index } => {
                self.observe_tool_call_start(name, index)
            }
            ConversationIntent::ObserveToolCall {
                id,
                name,
                index,
                summary,
            } => self.observe_tool_call(id, name, index, summary),
            ConversationIntent::ObserveToolResult {
                id,
                output,
                is_error,
            } => self.observe_tool_result(id, output, is_error),
            ConversationIntent::CompleteChat => self.complete_chat(),
            ConversationIntent::ObserveAssistantText { text } => {
                if let Some(chat) = self.active_chat_mut() {
                    if let Some(turn) = chat.active_turn_mut() {
                        turn.assistant_stream.push_str(&text);
                    }
                }
                Vec::new()
            }
            ConversationIntent::ObserveToolArguments {
                name,
                index,
                partial_args,
            } => {
                if let Some(chat) = self.active_chat_mut() {
                    if let Some(turn) = chat.active_turn_mut() {
                        if let Some(call) = turn
                            .tool_calls
                            .iter_mut()
                            .find(|call| call.stream_key.name == name && call.stream_key.index == index)
                        {
                            call.update_args(partial_args);
                        }
                    }
                }
                Vec::new()
            }
        }
    }

    fn start_chat(&mut self, submission: String) -> Vec<ConversationChange> {
        self.next_chat_sequence += 1;
        let chat_id = ChatId::new(format!("chat-{}", self.next_chat_sequence));
        let chat = Chat::new(chat_id.clone(), submission);
        self.active_chat_id = Some(chat_id.clone());
        self.chats.push(chat);
        vec![
            ConversationChange::ChatStarted {
                chat_id: chat_id.as_ref().to_string(),
            },
            ConversationChange::ChatTurnStarted {
                chat_id: chat_id.as_ref().to_string(),
                turn_id: "turn-1".to_string(),
            },
        ]
    }

    fn observe_tool_call_start(&mut self, name: String, index: usize) -> Vec<ConversationChange> {
        let Some(chat_id) = self.active_chat_id.clone() else {
            return Vec::new();
        };
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                turn.observe_tool_start(chat_id, name.clone(), index);
            }
        }
        vec![ConversationChange::ToolCallObserved { name, index }]
    }

    fn observe_tool_call(
        &mut self,
        id: String,
        name: String,
        index: usize,
        summary: String,
    ) -> Vec<ConversationChange> {
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                if turn.bind_tool(ToolCallId::new(id.clone()), &name, index, summary) {
                    return vec![ConversationChange::ToolCallBound { id, name }];
                }
            }
        }
        Vec::new()
    }

    fn observe_tool_result(
        &mut self,
        id: String,
        output: String,
        is_error: bool,
    ) -> Vec<ConversationChange> {
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                if let Some(status) = turn.complete_tool(&id, output, is_error) {
                    return vec![ConversationChange::ToolCallCompleted { id, status }];
                }
            }
        }
        vec![ConversationChange::OrphanToolResultObserved { id }]
    }

    fn complete_chat(&mut self) -> Vec<ConversationChange> {
        if let Some(chat) = self.active_chat_mut() {
            chat.status = ChatStatus::Completing;
            let chat_id = chat.id.as_ref().to_string();
            return vec![ConversationChange::ChatCompleting { chat_id }];
        }
        Vec::new()
    }

    fn active_chat_mut(&mut self) -> Option<&mut Chat> {
        let active = self.active_chat_id.clone()?;
        self.chats.iter_mut().find(|chat| chat.id == active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::intent::ConversationIntent;
    use crate::tui::model::conversation::tool_call::ToolCallStatus;

    #[test]
    fn test_conversation_observes_tool_lifecycle() {
        let mut model = ConversationModel::default();
        let changes = model.apply(ConversationIntent::StartChat {
            submission: "read file".to_string(),
        });
        assert!(changes
            .iter()
            .any(|change| matches!(change, ConversationChange::ChatStarted { .. })));

        model.apply(ConversationIntent::ObserveToolCallStart {
            name: "Read".to_string(),
            index: 0,
        });
        model.apply(ConversationIntent::ObserveToolCall {
            id: "tool-1".to_string(),
            name: "Read".to_string(),
            index: 0,
            summary: "Read file".to_string(),
        });
        let changes = model.apply(ConversationIntent::ObserveToolResult {
            id: "tool-1".to_string(),
            output: "ok".to_string(),
            is_error: false,
        });

        assert!(changes.iter().any(|change| matches!(
            change,
            ConversationChange::ToolCallCompleted { status, .. } if *status == ToolCallStatus::Success
        )));
    }

    #[test]
    fn test_conversation_reports_orphan_tool_result() {
        let mut model = ConversationModel::default();
        model.apply(ConversationIntent::StartChat {
            submission: "read file".to_string(),
        });
        let changes = model.apply(ConversationIntent::ObserveToolResult {
            id: "missing".to_string(),
            output: "late".to_string(),
            is_error: false,
        });
        assert!(changes.iter().any(|change| matches!(
            change,
            ConversationChange::OrphanToolResultObserved { id } if id == "missing"
        )));
    }
}
