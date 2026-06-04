use super::ids::{ToolCallId, ToolStreamKey};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCall {
    pub id: Option<ToolCallId>,
    pub stream_key: ToolStreamKey,
    pub name: String,
    pub args_preview: String,
    pub summary: Option<String>,
    pub status: ToolCallStatus,
    pub result: Option<String>,
    pub activities: Vec<String>,
}

impl ToolCall {
    pub fn pending(id: ToolCallId, stream_key: ToolStreamKey) -> Self {
        Self {
            name: stream_key.name.clone(),
            id: Some(id),
            stream_key,
            args_preview: String::new(),
            summary: None,
            status: ToolCallStatus::Running,
            result: None,
            activities: Vec::new(),
        }
    }
    pub fn update_args(&mut self, partial_args: impl Into<String>) {
        self.args_preview = partial_args.into();
    }

    pub fn bind(&mut self, summary: String) -> Vec<ToolCallChange> {
        self.summary = Some(summary);
        if self.status == ToolCallStatus::PendingArgs {
            self.status = ToolCallStatus::Running;
            vec![ToolCallChange::Bound, ToolCallChange::Running]
        } else {
            vec![ToolCallChange::Bound]
        }
    }
    pub fn complete(&mut self, result: String, is_error: bool) {
        self.result = Some(result);
        self.status = if is_error {
            ToolCallStatus::Error
        } else {
            ToolCallStatus::Success
        };
    }

    pub fn orphan(&mut self) {
        self.status = ToolCallStatus::Orphaned;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolCallStatus {
    PendingArgs,
    Ready,
    Running,
    Success,
    Error,
    Cancelled,
    Orphaned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolCallChange {
    Bound,
    Running,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};

    fn stream_key() -> ToolStreamKey {
        ToolStreamKey::new(ChatId::new("chat-1"), ChatTurnId::new("turn-1"), "Read", 0)
    }

    fn pending_call() -> ToolCall {
        ToolCall::pending(ToolCallId::new("tool-1"), stream_key())
    }

    #[test]
    fn test_tool_call_binds_id_and_runs() {
        let mut call = pending_call();
        let changes = call.bind("Read file".to_string());
        assert_eq!(call.id.as_ref().map(AsRef::as_ref), Some("tool-1"));
        assert_eq!(call.status, ToolCallStatus::Running);
        assert_eq!(changes, vec![ToolCallChange::Bound]);
    }

    #[test]
    fn test_tool_call_completes_success() {
        let mut call = pending_call();
        call.bind("Read file".to_string());
        call.complete("ok".to_string(), false);
        assert_eq!(call.status, ToolCallStatus::Success);
        assert_eq!(call.result.as_deref(), Some("ok"));
    }

    #[test]
    fn test_tool_call_completes_error() {
        let mut call = pending_call();
        call.bind("Read file".to_string());
        call.complete("failed".to_string(), true);
        assert_eq!(call.status, ToolCallStatus::Error);
        assert_eq!(call.result.as_deref(), Some("failed"));
    }
}
