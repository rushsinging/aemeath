//! Chat 输入 / 事件 / 流 / 结果。

use crate::{ChatInputEventPort, ChatMessage, QueueDrainPort};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;

/// AskUserQuestion 选项项：简要 title + 详细 description。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Hash)]
pub struct OptionItem {
    /// 简要标题（必填）。
    pub title: String,
    /// 详细描述（可选）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl<'de> Deserialize<'de> for OptionItem {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de;

        #[derive(Deserialize)]
        struct Obj {
            title: String,
            #[serde(default)]
            description: Option<String>,
        }

        // 先尝试按对象反序列化
        let value = serde_json::Value::deserialize(de)?;
        if value.is_string() {
            Ok(OptionItem::title_only(value.as_str().unwrap().to_string()))
        } else if value.is_object() {
            let obj: Obj =
                serde_json::from_value(value).map_err(|e| de::Error::custom(e.to_string()))?;
            Ok(OptionItem {
                title: obj.title,
                description: obj.description,
            })
        } else {
            Err(de::Error::custom(
                "expected string or object { title, description }",
            ))
        }
    }
}

impl OptionItem {
    pub fn title_only(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
        }
    }

    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: Some(description.into()),
        }
    }
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
    UserMessage {
        text: String,
        image_paths: Vec<String>,
    },
    /// 忙碌期间输入的 slash/control command，永不作为 user message 发给 LLM。
    ControlCommand { raw: String },
    /// 用户请求取消当前 Chat；与现有 cancel token 幂等合流。
    Cancel,
}

impl ChatInputEvent {
    pub fn user_message(text: impl Into<String>, image_paths: Vec<String>) -> Self {
        Self::UserMessage {
            text: text.into(),
            image_paths,
        }
    }

    pub fn classify_text(text: impl Into<String>, image_paths: Vec<String>) -> Self {
        let text = text.into();
        if text.trim_start().starts_with('/') {
            Self::ControlCommand { raw: text }
        } else {
            Self::UserMessage { text, image_paths }
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

/// 工具结果中的图片载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultImage {
    pub base64: String,
    pub media_type: String,
}

/// Sub-agent 工具调用进度。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentToolCallProgressView {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub summary: String,
}

impl std::fmt::Display for AgentToolCallProgressView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.name, self.summary)
    }
}

/// Sub-agent 进度类型。
#[derive(Debug, Clone, PartialEq)]
pub enum AgentProgressKindView {
    Message {
        text: String,
    },
    ToolCalls {
        calls: Vec<AgentToolCallProgressView>,
    },
}

impl std::fmt::Display for AgentProgressKindView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message { text } => write!(f, "{text}"),
            Self::ToolCalls { calls } => {
                for (i, call) in calls.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{call}")?;
                }
                Ok(())
            }
        }
    }
}

/// Sub-agent 进度事件。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentProgressEventView {
    pub sequence: usize,
    pub kind: AgentProgressKindView,
}

impl std::fmt::Display for AgentProgressEventView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)
    }
}

/// workspace 栈条目视图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceStackEntryView {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}

/// TUI 可展示的 workspace 上下文视图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceContextView {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
    pub context_stack: Vec<WorkspaceStackEntryView>,
}

/// Hook 执行状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookEventStatus {
    Running,
    Succeeded,
    Blocked,
    Failed,
}

/// Hook 执行结果视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookExecutionResultView {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub decision: Option<String>,
    pub reason: Option<String>,
    pub additional_context: Option<String>,
}

/// Hook 事件视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEventView {
    pub hook_name: String,
    pub status: HookEventStatus,
    pub matcher: Option<String>,
    pub command: Option<String>,
    pub result: Option<HookExecutionResultView>,
}

/// Runtime stream context used to bind UI events to the authoritative chat/turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEventContext {
    pub chat_id: String,
    pub turn_id: String,
}

impl ChatEventContext {
    pub fn new(chat_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            chat_id: chat_id.into(),
            turn_id: turn_id.into(),
        }
    }
}

/// Chat 事件流中的单个事件。
#[derive(Debug)]
pub enum ChatEvent {
    /// LLM 返回的文本 token。
    Token {
        context: ChatEventContext,
        text: String,
    },
    /// LLM reasoning / thinking token。
    Thinking {
        context: ChatEventContext,
        text: String,
    },
    /// 文本块完成。
    TextBlockComplete {
        context: ChatEventContext,
        text: String,
    },
    /// 工具调用开始。
    ToolCallStart {
        context: ChatEventContext,
        id: String,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    /// 工具参数增量。
    ToolArgumentsDelta {
        context: ChatEventContext,
        id: String,
        provider_id: Option<String>,
        index: usize,
        name: String,
        partial_args: String,
    },
    /// 工具调用确认。
    ToolCall {
        context: ChatEventContext,
        id: String,
        provider_id: String,
        name: String,
        index: Option<usize>,
        summary: String,
    },
    /// 工具执行结果。
    ToolResult {
        context: ChatEventContext,
        id: String,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        images: Vec<ToolResultImage>,
    },
    /// 系统消息。
    SystemMessage(String),
    /// Chat 出错。
    Error(String),
    /// 用量统计。
    Usage {
        input: u32,
        output: u32,
        last_input: u32,
        elapsed_secs: f64,
    },
    /// runtime 同步当前 messages。
    MessagesSync(Vec<ChatMessage>),
    /// Chat 完成。
    Done,
    /// Chat 完成并附带耗时毫秒。
    DoneWithDurationMs(u64),
    /// Chat 被取消。
    Cancelled,
    /// 实时 TPS。
    LiveTps(f64),
    /// 当前 turn 变化。
    TurnChanged(usize),
    /// 记录当前 turn 变化的端口事件。
    CurrentTurnChanged(usize),
    /// Hook 事件。
    HookEvent(HookEventView),
    /// AskUserQuestion 请求。
    AskUser {
        id: String,
        question: String,
        options: Vec<OptionItem>,
        allow_free_input: bool,
        multi_select: bool,
        default: Option<String>,
        reply_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// Agent progress 事件投影。
    AgentProgress {
        tool_id: String,
        event: AgentProgressEventView,
    },
    /// 工作目录变化。
    WorkingDirectoryChanged {
        path_base: String,
        working_root: String,
        workspace: WorkspaceContextView,
    },
    /// 任务列表状态发生变化，TUI 应重新拉取 task_status 快照。
    TasksChanged,
    /// 兼容旧 ChatInput 流结果。
    Result(ChatResult),
}

/// Chat 完成结果。
#[derive(Debug, Clone)]
pub struct ChatResult {
    /// 最终响应文本。
    pub text: String,
    /// 本次 Chat 消耗的 token 数（如果可用）。
    pub tokens_used: Option<u64>,
}

/// Chat 事件流。
///
/// TUI 使用 `recv().await` 阻塞等待——终端事件循环是轮询模型。
pub struct ChatStream {
    rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent>,
}

impl ChatStream {
    pub fn new(rx: tokio::sync::mpsc::UnboundedReceiver<ChatEvent>) -> Self {
        Self { rx }
    }

    /// 接收下一个事件，流结束时返回 None。
    pub async fn recv(&mut self) -> Option<ChatEvent> {
        self.rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chat_stream_recv_returns_sent_event() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(ChatEvent::Token {
            context: ChatEventContext::new("chat-1", "turn-1"),
            text: "hello".to_string(),
        })
        .unwrap();
        drop(tx);
        let mut stream = ChatStream::new(rx);

        let event = stream.recv().await;

        match event {
            Some(ChatEvent::Token { context, text }) => {
                assert_eq!(context.chat_id, "chat-1");
                assert_eq!(context.turn_id, "turn-1");
                assert_eq!(text, "hello");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_chat_stream_recv_returns_none_after_sender_dropped() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(_tx);
        let mut stream = ChatStream::new(rx);

        assert!(stream.recv().await.is_none());
    }

    #[test]
    fn test_tool_result_image_keeps_base64_and_media_type() {
        let image = ToolResultImage {
            base64: "abc".to_string(),
            media_type: "image/png".to_string(),
        };

        assert_eq!(image.base64, "abc");
        assert_eq!(image.media_type, "image/png");
    }

    #[test]
    fn test_agent_progress_view_supports_message_and_tool_calls() {
        let message = AgentProgressEventView {
            sequence: 1,
            kind: AgentProgressKindView::Message {
                text: "working".to_string(),
            },
        };
        let tools = AgentProgressEventView {
            sequence: 2,
            kind: AgentProgressKindView::ToolCalls {
                calls: vec![AgentToolCallProgressView {
                    id: "tool-1".to_string(),
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path":"a.rs"}),
                    summary: "a.rs".to_string(),
                }],
            },
        };

        assert_eq!(message.sequence, 1);
        match message.kind {
            AgentProgressKindView::Message { text } => assert_eq!(text, "working"),
            other => panic!("unexpected kind: {other:?}"),
        }
        match tools.kind {
            AgentProgressKindView::ToolCalls { calls } => {
                assert_eq!(calls[0].name, "Read");
                assert_eq!(calls[0].summary, "a.rs");
            }
            other => panic!("unexpected kind: {other:?}"),
        }
    }

    #[test]
    fn test_agent_progress_display_tool_calls() {
        let event = AgentProgressEventView {
            sequence: 1,
            kind: AgentProgressKindView::ToolCalls {
                calls: vec![
                    AgentToolCallProgressView {
                        id: "c1".to_string(),
                        name: "Bash".to_string(),
                        input: serde_json::json!({"command": "ls"}),
                        summary: "ls -la /project".to_string(),
                    },
                    AgentToolCallProgressView {
                        id: "c2".to_string(),
                        name: "Read".to_string(),
                        input: serde_json::json!({"file_path": "TODO.md"}),
                        summary: "project/TODO.md".to_string(),
                    },
                ],
            },
        };
        assert_eq!(
            format!("{event}"),
            "Bash ls -la /project, Read project/TODO.md"
        );
    }

    #[test]
    fn test_agent_progress_display_message() {
        let event = AgentProgressEventView {
            sequence: 2,
            kind: AgentProgressKindView::Message {
                text: "分析完成".to_string(),
            },
        };
        assert_eq!(format!("{event}"), "分析完成");
    }

    #[test]
    fn test_workspace_context_view_keeps_paths() {
        let view = WorkspaceContextView {
            path_base: "/repo/sub".into(),
            working_root: "/repo".into(),
            context_stack: vec![WorkspaceStackEntryView {
                path_base: "/repo".into(),
                working_root: "/repo".into(),
            }],
        };

        assert_eq!(view.path_base.to_string_lossy(), "/repo/sub");
        assert_eq!(view.working_root.to_string_lossy(), "/repo");
        assert_eq!(view.context_stack.len(), 1);
    }

    #[test]
    fn test_chat_request_keeps_message_order() {
        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: "user".to_string(),
                    content: serde_json::json!([{"type":"text","text":"one"}]),
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    content: serde_json::json!([{"type":"text","text":"two"}]),
                },
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
        let event = ChatInputEvent::classify_text("继续分析", vec!["a.png".to_string()]);
        assert!(matches!(
            event,
            ChatInputEvent::UserMessage { ref text, ref image_paths }
                if text == "继续分析" && image_paths == &["a.png".to_string()]
        ));
    }

    #[test]
    fn test_chat_input_event_classify_text_control_command() {
        let event = ChatInputEvent::classify_text("  /clear", vec!["ignored.png".to_string()]);
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
    fn test_option_item_title_only() {
        let item = OptionItem::title_only("Yes".to_string());
        assert_eq!(item.title, "Yes");
        assert!(item.description.is_none());
    }

    #[test]
    fn test_option_item_with_description() {
        let item = OptionItem::new("Deploy", "Push to production");
        assert_eq!(item.title, "Deploy");
        assert_eq!(item.description.as_deref(), Some("Push to production"));
    }

    #[test]
    fn test_option_item_serialize_deserialize_string_compat() {
        // 向后兼容：纯字符串应反序列化为 title_only
        let json = serde_json::json!("Simple option");
        let item: OptionItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.title, "Simple option");
        assert!(item.description.is_none());
    }

    #[test]
    fn test_option_item_serialize_deserialize_object() {
        let json = serde_json::json!({"title": "Go", "description": "Proceed"});
        let item: OptionItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.title, "Go");
        assert_eq!(item.description, Some("Proceed".to_string()));
    }

    #[test]
    fn test_option_item_serialize_outputs_object() {
        let item = OptionItem::new("Test", "Desc");
        let val = serde_json::to_value(&item).unwrap();
        assert_eq!(val["title"], "Test");
        assert_eq!(val["description"], "Desc");
    }
}
