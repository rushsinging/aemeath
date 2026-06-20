//! Chat 输入 / 请求类型与重导出。

use crate::{ChatInputEventPort, ChatMessage, QueueDrainPort};

pub use crate::chat_event::{AddedInput, ChatEvent, ChatEventContext, ToolCallStatusView};
pub use crate::chat_result::{ChatResult, ChatStream, ToolResultImage};
pub use crate::chat_view::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, HookEventStatus,
    HookEventView, HookExecutionResultView, OptionItem, WorkspaceContextView,
    WorkspaceStackEntryView,
};

/// AskUserQuestion 批量事件中的单个问题项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskUserQuestionItem {
    /// 对应的 tool_call_id（用于 TUI 关联 ToolCall 状态）。
    pub id: String,
    /// 问题文本。
    pub question: String,
    /// 预设选项（LLM 选项，不含内建选项）。
    pub options: Vec<OptionItem>,
    /// 是否多选。
    pub multi_select: bool,
    /// 默认值（用户跳过时使用）。
    pub default: Option<String>,
}

/// 用户发送给 Agent 的一次 Chat 输入。
#[derive(Debug, Clone)]
pub struct ChatInput {
    pub text: String,
    /// 附加图片路径（可选）。
    pub image_paths: Vec<String>,
}

/// Chat 运行期间追加到 runtime 的输入事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatInputEvent {
    /// 普通用户消息，延展当前 Chat 为新的 Turn。
    ///
    /// `images` 携带图片数据（base64 + media_type），使内联/粘贴/文件图片均能
    /// 经事件通道存活到达 LLM（#402：A1 之前只带 display_path，内联图被丢）。
    UserMessage {
        text: String,
        images: Vec<crate::ToolResultImage>,
    },
    /// 忙碌期间输入的 slash/control command，永不作为 user message 发给 LLM。
    ControlCommand { raw: String },
    /// 用户请求取消当前 Chat；与现有 cancel token 幂等合流。
    Cancel,
}

impl ChatInputEvent {
    pub fn user_message(text: impl Into<String>, images: Vec<crate::ToolResultImage>) -> Self {
        Self::UserMessage {
            text: text.into(),
            images,
        }
    }

    pub fn classify_text(text: impl Into<String>, images: Vec<crate::ToolResultImage>) -> Self {
        let text = text.into();
        if text.trim_start().starts_with('/') {
            Self::ControlCommand { raw: text }
        } else {
            Self::UserMessage { text, images }
        }
    }
}

/// TUI 发起的一次 Chat 请求。
#[derive(Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub queue_drain: Option<std::sync::Arc<dyn QueueDrainPort>>,
    pub input_events: Option<std::sync::Arc<dyn ChatInputEventPort>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_request_keeps_message_order() {
        let request = ChatRequest {
            messages: vec![
                ChatMessage::user_text("one"),
                ChatMessage::assistant_text("two"),
            ],
            queue_drain: None,
            input_events: None,
        };

        assert_eq!(request.messages[0].role, "user");
        assert_eq!(request.messages[1].role, "assistant");
        assert!(request.queue_drain.is_none());
        assert!(request.input_events.is_none());
    }

    #[test]
    fn test_chat_input_event_classify_text_user_message() {
        let img = crate::ToolResultImage {
            base64: "AAAA".to_string(),
            media_type: "image/png".to_string(),
        };
        let event = ChatInputEvent::classify_text("继续分析", vec![img.clone()]);
        match event {
            ChatInputEvent::UserMessage { text, images } => {
                assert_eq!(text, "继续分析");
                assert_eq!(images, vec![img]);
            }
            other => panic!("expected UserMessage, got {other:?}"),
        }
    }

    #[test]
    fn test_chat_input_event_classify_text_control_command() {
        let img = crate::ToolResultImage {
            base64: "x".to_string(),
            media_type: "image/png".to_string(),
        };
        let event = ChatInputEvent::classify_text("  /clear", vec![img]);
        assert!(matches!(
            event,
            ChatInputEvent::ControlCommand { ref raw } if raw == "  /clear"
        ));
    }

    #[test]
    fn test_chat_input_event_cancel_is_distinct_from_user_message() {
        assert_eq!(ChatInputEvent::Cancel, ChatInputEvent::Cancel);
        assert_ne!(
            ChatInputEvent::Cancel,
            ChatInputEvent::user_message("cancel", Vec::new())
        );
    }
}
