use std::borrow::Cow;

use crate::tui::model::conversation::block::HookNoticeContent;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TimelineRuntimeContext {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}

impl TimelineRuntimeContext {
    pub fn new(chat_id: ChatId, turn_id: ChatTurnId) -> Self {
        Self { chat_id, turn_id }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TimelineToolCallRef {
    pub context: TimelineRuntimeContext,
    pub tool_call_id: ToolCallId,
}

impl TimelineToolCallRef {
    pub fn new(chat_id: ChatId, turn_id: ChatTurnId, tool_call_id: ToolCallId) -> Self {
        Self {
            context: TimelineRuntimeContext::new(chat_id, turn_id),
            tool_call_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputTimelineItem {
    UserMessage {
        id: String,
        text: String,
    },
    AssistantText {
        id: String,
        context: Option<TimelineRuntimeContext>,
        text: String,
    },
    Thinking {
        id: String,
        context: Option<TimelineRuntimeContext>,
        text: String,
    },
    ToolCall {
        reference: TimelineToolCallRef,
    },
    ToolResult {
        reference: TimelineToolCallRef,
    },
    System {
        id: String,
        text: String,
    },
    HookNotice {
        id: String,
        content: HookNoticeContent,
    },
    Error {
        id: String,
        text: String,
    },
    QueuedUserMessage {
        id: String,
        text: String,
    },
    AgentProgress {
        id: String,
        tool_id: ToolCallId,
        message: String,
    },
    OrphanToolResult {
        id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
    },
    AskUserBatch {
        id: String,
        slots: Vec<crate::tui::model::conversation::block::AskUserSlot>,
        active_index: usize,
        phase: crate::tui::model::conversation::block::AskUserPhase,
        cursor: usize,
        selected: Vec<bool>,
        chat_input_active: bool,
        chat_input_text: String,
        confirm_cursor: usize,
        confirmed: bool,
    },
}

impl OutputTimelineItem {
    pub fn id(&self) -> Cow<'_, str> {
        match self {
            OutputTimelineItem::UserMessage { id, .. }
            | OutputTimelineItem::AssistantText { id, .. }
            | OutputTimelineItem::Thinking { id, .. }
            | OutputTimelineItem::System { id, .. }
            | OutputTimelineItem::HookNotice { id, .. }
            | OutputTimelineItem::Error { id, .. }
            | OutputTimelineItem::QueuedUserMessage { id, .. }
            | OutputTimelineItem::AgentProgress { id, .. }
            | OutputTimelineItem::OrphanToolResult { id, .. }
            | OutputTimelineItem::AskUserBatch { id, .. } => Cow::Borrowed(id),
            OutputTimelineItem::ToolCall { reference } => {
                Cow::Owned(format!("tool-call-{}", reference.tool_call_id.as_ref()))
            }
            OutputTimelineItem::ToolResult { reference } => {
                Cow::Owned(format!("tool-result-{}", reference.tool_call_id.as_ref()))
            }
        }
    }

    pub fn is_tool_owned_payload_free(&self) -> bool {
        matches!(
            self,
            OutputTimelineItem::ToolCall { .. } | OutputTimelineItem::ToolResult { .. }
        )
    }
}
