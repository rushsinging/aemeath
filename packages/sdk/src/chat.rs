//! Chat 输入 / 请求类型与重导出。

use crate::ChatInputEventPort;

pub use crate::chat_event::{
    ChatEvent, ChatEventContext, ReflectionApplyStatusView, ReflectionErrorCategoryView,
    ReflectionHistoryView, ReflectionStatusView, ReflectionTokenUsageView, ReflectionTriggerView,
    ResumedSessionStep, ResumedStepFinalizeCause, RunStatusView, ToolCallStatusView,
};
pub use crate::chat_result::{ChatInputImage, ChatResult, ChatStream, ToolResultImage};
pub use crate::chat_view::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, HookEventStatus,
    HookEventView, HookExecutionResultView, HookMessageKindView, HookMessageView, OptionItem,
    WorkspaceContextView, WorkspaceStackEntryView,
};

/// AskUserQuestion 批量事件中的单个问题项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskUserQuestionItem {
    /// 对应的 tool_call_id（用于 TUI 关联 ToolCall 状态）。
    pub id: String,
    /// 同一 tool call 内的问题序号，从 0 开始。
    pub question_seq: usize,
    /// 问题文本。
    pub question: String,
    /// 预设选项（LLM 选项，不含内建选项）。
    pub options: Vec<OptionItem>,
    /// 是否多选。
    pub multi_select: bool,
    /// 是否允许自由输入。
    pub allow_free_input: bool,
    /// 默认值（用户跳过时使用）。
    pub default: Option<String>,
}

/// 带身份的 AskUserQuestion 回答。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskUserAnswer {
    pub tool_call_id: String,
    pub question_seq: usize,
    pub answer: String,
}

/// AskUser 批量交互的显式终结结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskUserReply {
    Answers(Vec<AskUserAnswer>),
    Cancelled,
}

/// 用户发送给 Agent 的一次 Chat 输入。
#[derive(Debug, Clone)]
pub struct ChatInput {
    pub text: String,
    /// 附加图片路径（可选）。
    pub image_paths: Vec<String>,
}

/// TUI 请求 Runtime 将一次 Skill 调用意图交给 LLM。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRequest {
    pub input_id: crate::InputId,
    pub skill: String,
    pub arguments: String,
    pub raw_input: String,
}

/// Chat 运行期间追加到 runtime 的输入事件。
#[derive(Debug, Clone)]
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
    SkillRequest(SkillRequest),
    /// 忙碌期间输入的 slash/control command，永不作为 user message 发给 LLM。
    ControlCommand {
        raw: String,
    },
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
    /// 用户请求切换模型：idle 时立即执行，busy 时排队等回合结束后执行。
    ///
    /// 由 `/model` 触发，走 runtime 事件流（#567）。`selection` 是用户输入的
    /// `Provider/Model` 字符串，由 runtime 侧 `resolve_model_selection` 解析。
    /// 结果通过 `ModelSwitched` 事件回传 TUI。
    SwitchModel {
        selection: String,
    },
    /// 用户请求切换 reasoning 模式：idle 时立即执行，busy 时排队等回合结束后执行。
    ///
    /// 由 `/think` 触发，走 runtime 事件流（#497）。`desired = None` 表示 toggle。
    /// runtime idle 分支更新会话级 reasoning 状态，后续调用通过不可变 InvocationScope 读取；
    /// 结果通过 `ThinkingChanged` 事件回传 TUI。
    SetThinking {
        desired: Option<bool>,
    },
    /// 初始化项目。由 `/init` 触发。
    /// force = true 时强制重新初始化
    InitProject {
        force: bool,
    },
    /// 管理会话。由 `/session` 触发。
    /// args: "" / "list" / "new" / "rename <id> <name>" / "delete <id>" / "export <id>" / "import <file>"
    ManageSession {
        args: String,
    },
    /// 管理记忆。由 `/memory` 触发（非 remind 子命令）。
    /// args: "" / "list" / "add ..." / "delete ..." / "pin ..." / "search ..." / "compact" / "stats"
    ManageMemory {
        args: String,
    },
    /// 恢复指定会话。由 `/resume <id>` 触发。
    /// 需要走 idle gate（替换 loop messages）。
    ResumeSession {
        id: String,
    },
    /// 查询最近的 reflection 历史。由 `/reflect [limit]` 触发；不运行或应用 reflection。
    QueryReflectionHistory {
        limit: usize,
    },
    /// 查询可用模型列表。由 TUI 启动时或 `/model` 触发。
    ListModels,
    /// 查询提醒列表。由 `/reminders` 触发。
    ListReminders,
}

// #567: 手动实现 PartialEq/Eq，不比较变体内数据（测试只检查是否产生了事件）。
impl PartialEq for ChatInputEvent {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
impl Eq for ChatInputEvent {}

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
    /// Session 唯一 typed ingress；首次、后续、命令和 Skill 输入均走此端口。
    pub ingress: std::sync::Arc<dyn ChatInputEventPort>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_request_preserves_identity_and_raw_arguments() {
        let input_id = crate::InputId::new_v7();
        let event = ChatInputEvent::SkillRequest(SkillRequest {
            input_id: input_id.clone(),
            skill: "release".to_string(),
            arguments: "v1.2.3 --dry-run".to_string(),
            raw_input: "/release v1.2.3 --dry-run".to_string(),
        });
        match event {
            ChatInputEvent::SkillRequest(request) => {
                assert_eq!(request.input_id, input_id);
                assert_eq!(request.skill, "release");
                assert_eq!(request.arguments, "v1.2.3 --dry-run");
                assert_eq!(request.raw_input, "/release v1.2.3 --dry-run");
            }
            other => panic!("expected SkillRequest, got {other:?}"),
        }
    }

    #[derive(Default)]
    struct ClosedInputPort;

    impl crate::ChatInputEventPort for ClosedInputPort {
        fn drain_input_events<'a>(&'a self) -> crate::InputEventFuture<'a> {
            Box::pin(async { Vec::new() })
        }

        fn recv_next<'a>(&'a self) -> crate::InputEventOptFuture<'a> {
            Box::pin(async { None })
        }
    }

    #[test]
    fn chat_request_requires_single_typed_ingress() {
        let request = ChatRequest {
            ingress: std::sync::Arc::new(ClosedInputPort),
        };

        let _ingress = request.ingress;
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
    fn test_user_message_generates_v7_input_id() {
        match ChatInputEvent::user_message("x", vec![]) {
            ChatInputEvent::UserMessage { id, .. } => {
                assert_eq!(id.as_uuid().get_version_num(), 7);
            }
            other => panic!("expected UserMessage, got {other:?}"),
        }
    }
}
