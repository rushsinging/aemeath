//! Chat 输入 / 请求类型与重导出。

use crate::{ChatInputEventPort, ChatMessage, QueueDrainPort};

pub use crate::chat_event::{AddedInput, ChatEvent, ChatEventContext, ToolCallStatusView};
pub use crate::chat_result::{ChatInputImage, ChatResult, ChatStream, ToolResultImage};
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
    /// `id` 是唯一标识本次输入的 UUIDv7（#390 A2）。
    /// `images` 携带图片数据（`ChatInputImage` 含 `id` 占位符），使内联/粘贴/文件
    /// 图片均能经事件通道存活到达 LLM（#402 + #fix-tui-image-input-output）：
    /// `text` 中出现的 `[Image #N]` ↔ `images[i].id == "[Image #N]"`（1-based ↔ 0-based）。
    UserMessage {
        id: crate::InputId,
        text: String,
        images: Vec<crate::ChatInputImage>,
    },
    /// 忙碌期间输入的 slash/control command，永不作为 user message 发给 LLM。
    ControlCommand { raw: String },
    /// 用户请求取消当前 Chat；与现有 cancel token 幂等合流。
    Cancel,
    /// 整段会话重置：清空 messages + pending 输入，通知 TUI。
    ///
    /// 由 `/clear` 触发（idle 立即执行 / busy 排队等当前回合自然结束回 idle gate 后执行），
    /// **不打断当前回合**（NEVER 调 CancellationToken）。
    Reset,
    /// 批量撤回所有 pending 输入：清空 PendingInputBuffer + 回传 texts 还原输入框。
    ///
    /// 由 busy 态 Up 键触发（#391 S3）。
    WithdrawAll,
    /// 用户请求手动 compact：idle 时立即执行，busy 时排队等回合结束后执行。
    ///
    /// 由 `/compact` 触发，走 runtime 事件流（#497），不再调 `compact_messages()` trait。
    Compact,
}

impl ChatInputEvent {
    pub fn user_message(text: impl Into<String>, images: Vec<crate::ChatInputImage>) -> Self {
        Self::UserMessage {
            id: crate::InputId::new_v7(),
            text: text.into(),
            images,
        }
    }

    pub fn classify_text(text: impl Into<String>, images: Vec<crate::ChatInputImage>) -> Self {
        let text = text.into();
        if text.trim_start().starts_with('/') {
            Self::ControlCommand { raw: text }
        } else {
            Self::UserMessage {
                id: crate::InputId::new_v7(),
                text,
                images,
            }
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
        let img = crate::ChatInputImage {
            id: "[Image #1]".to_string(),
            base64: "AAAA".to_string(),
            media_type: "image/png".to_string(),
        };
        let event = ChatInputEvent::classify_text("继续分析", vec![img.clone()]);
        match event {
            ChatInputEvent::UserMessage { text, images, .. } => {
                assert_eq!(text, "继续分析");
                assert_eq!(images, vec![img]);
            }
            other => panic!("expected UserMessage, got {other:?}"),
        }
    }

    #[test]
    fn test_chat_input_event_classify_text_control_command() {
        let img = crate::ChatInputImage {
            id: "[Image #1]".to_string(),
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

    #[test]
    fn test_user_message_generates_v7_input_id() {
        match ChatInputEvent::user_message("x", vec![]) {
            ChatInputEvent::UserMessage { id, .. } => {
                assert_eq!(id.as_uuid().get_version_num(), 7);
            }
            other => panic!("expected UserMessage, got {other:?}"),
        }
    }
}
