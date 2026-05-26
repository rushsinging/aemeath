//! Chat 输入 / 事件 / 流 / 结果。

use crate::ChatMessage;
use std::path::PathBuf;

/// 用户发送给 Agent 的一次 Chat 输入。
#[derive(Debug, Clone)]
pub struct ChatInput {
    pub text: String,
    /// 附加图片路径（可选）。
    pub image_paths: Vec<String>,
}

/// TUI 发起的一次 Chat 请求。
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
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

/// Sub-agent 进度事件。
#[derive(Debug, Clone, PartialEq)]
pub struct AgentProgressEventView {
    pub sequence: usize,
    pub kind: AgentProgressKindView,
}

/// workspace 栈条目视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceStackEntryView {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}

/// TUI 可展示的 workspace 上下文视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceContextView {
    pub path_base: PathBuf,
    pub working_root: PathBuf,
    pub context_stack: Vec<WorkspaceStackEntryView>,
}

/// Chat 事件流中的单个事件。
#[derive(Debug)]
pub enum ChatEvent {
    /// LLM 返回的文本 token。
    Token(String),
    /// LLM reasoning / thinking token。
    Thinking(String),
    /// 文本块完成。
    TextBlockComplete(String),
    /// 工具调用开始。
    ToolCallStart { name: String, index: usize },
    /// 工具参数增量。
    ToolArgumentsDelta {
        index: usize,
        name: String,
        partial_args: String,
    },
    /// 工具调用确认。
    ToolCall {
        id: String,
        name: String,
        summary: String,
    },
    /// 工具执行结果。
    ToolResult {
        id: String,
        tool_name: String,
        output: String,
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
    /// StopFailure hook 结果。
    StopFailureHook {
        system_message: Option<String>,
        additional_context: Option<String>,
    },
    /// AskUserQuestion 请求。
    AskUser {
        id: String,
        question: String,
        options: Vec<String>,
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
    /// Hook 开始。
    HookStart { event: String, command: String },
    /// Hook 结束。
    HookEnd {
        event: String,
        blocked: bool,
        error: Option<String>,
    },
    /// 工作目录变化。
    WorkingDirectoryChanged {
        path_base: String,
        working_root: String,
        workspace: WorkspaceContextView,
    },
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
        tx.send(ChatEvent::Token("hello".to_string())).unwrap();
        drop(tx);
        let mut stream = ChatStream::new(rx);

        let event = stream.recv().await;

        match event {
            Some(ChatEvent::Token(text)) => assert_eq!(text, "hello"),
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
        };

        assert_eq!(request.messages[0].role, "user");
        assert_eq!(request.messages[1].role, "assistant");
    }
}
