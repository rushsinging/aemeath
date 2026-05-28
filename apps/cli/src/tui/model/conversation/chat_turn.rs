use super::ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};
use super::tool_call::{ToolCall, ToolCallStatus};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatTurn {
    pub id: ChatTurnId,
    pub sequence: usize,
    pub status: ChatTurnStatus,
    pub assistant_stream: String,
    pub tool_calls: Vec<ToolCall>,
}

impl ChatTurn {
    pub fn new(id: ChatTurnId, sequence: usize) -> Self {
        Self {
            id,
            sequence,
            status: ChatTurnStatus::Streaming,
            assistant_stream: String::new(),
            tool_calls: Vec::new(),
        }
    }

    pub fn observe_tool_start(&mut self, chat_id: ChatId, name: String, index: usize) {
        let key = ToolStreamKey::new(chat_id, self.id.clone(), name, index);
        self.tool_calls.push(ToolCall::pending(key));
        self.status = ChatTurnStatus::ToolCalling;
    }

    pub fn bind_tool(
        &mut self,
        id: ToolCallId,
        name: &str,
        index: usize,
        summary: String,
    ) -> Option<String> {
        if let Some(call) = self
            .tool_calls
            .iter_mut()
            .find(|call| call.stream_key.name == name && call.stream_key.index == index)
        {
            call.bind(id, summary);
            self.status = ChatTurnStatus::ToolExecuting;
            return Some(call.args_preview.clone());
        }
        None
    }

    pub fn complete_tool(
        &mut self,
        id: &str,
        output: String,
        is_error: bool,
    ) -> Option<ToolCallStatus> {
        let call = self
            .tool_calls
            .iter_mut()
            .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id))?;
        call.complete(output, is_error);
        let status = call.status;
        if self.tool_calls.iter().all(|call| {
            matches!(
                call.status,
                ToolCallStatus::Success
                    | ToolCallStatus::Error
                    | ToolCallStatus::Cancelled
                    | ToolCallStatus::Orphaned
            )
        }) {
            self.status = ChatTurnStatus::Completing;
        }
        Some(status)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatTurnStatus {
    Streaming,
    ToolCalling,
    ToolExecuting,
    Completing,
    Completed,
    Failed,
}
