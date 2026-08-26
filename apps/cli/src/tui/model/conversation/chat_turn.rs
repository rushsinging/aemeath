use super::ids::{ChatId, ChatRunId, ToolCallId, ToolStreamKey};
use super::tool_call::{ToolCall, ToolCallChange, ToolCallStatus};
use super::tool_result_payload::ToolResultPayload;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatRun {
    pub id: ChatRunId,
    pub sequence: usize,
    pub status: ChatTurnStatus,
    pub assistant_stream: String,
    pub tool_calls: Vec<ToolCall>,
}

impl ChatRun {
    pub fn new(id: ChatRunId, sequence: usize) -> Self {
        Self {
            id,
            sequence,
            status: ChatTurnStatus::Streaming,
            assistant_stream: String::new(),
            tool_calls: Vec::new(),
        }
    }

    pub fn observe_tool_start(
        &mut self,
        id: ToolCallId,
        chat_id: ChatId,
        name: String,
        index: usize,
    ) {
        let key = ToolStreamKey::new(chat_id, self.id.clone(), name, index);
        self.tool_calls.push(ToolCall::pending(id, key));
        self.status = ChatTurnStatus::ToolExecuting;
    }

    pub fn update_tool(
        &mut self,
        id: &str,
        arguments: Option<String>,
        status: ToolCallStatus,
    ) -> Option<(String, Vec<ToolCallChange>)> {
        let call = self
            .tool_calls
            .iter_mut()
            .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id))?;
        let changes = call.update(arguments, status);
        self.status = match status {
            ToolCallStatus::PendingArgs | ToolCallStatus::Ready => {
                if self.status == ChatTurnStatus::Completed {
                    self.status
                } else {
                    ChatTurnStatus::ToolCalling
                }
            }
            ToolCallStatus::Running => ChatTurnStatus::ToolExecuting,
            ToolCallStatus::Success | ToolCallStatus::Error | ToolCallStatus::Cancelled => {
                self.status
            }
        };
        Some((call.args_preview.clone(), changes))
    }
    pub fn complete_tool(&mut self, id: &str, result: ToolResultPayload) -> Option<ToolCallStatus> {
        let call = self
            .tool_calls
            .iter_mut()
            .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id))?;
        call.complete(result);
        let status = call.status;
        if self.tool_calls.iter().all(|call| {
            matches!(
                call.status,
                ToolCallStatus::Success | ToolCallStatus::Error | ToolCallStatus::Cancelled
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
}
