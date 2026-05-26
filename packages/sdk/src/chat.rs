//! Chat 输入 / 事件 / 流 / 结果。

/// 用户发送给 Agent 的一次 Chat 输入。
#[derive(Debug, Clone)]
pub struct ChatInput {
    pub text: String,
    /// 附加图片路径（可选）。
    pub image_paths: Vec<String>,
}

/// Chat 事件流中的单个事件。
#[derive(Debug, Clone)]
pub enum ChatEvent {
    /// LLM 返回的文本 token。
    Token(String),
    /// 工具调用开始。
    ToolCallStart { id: String, name: String },
    /// 工具调用增量输出。
    ToolCallDelta { id: String, delta: String },
    /// 工具调用结束。
    ToolCallEnd { id: String },
    /// 工具执行结果。
    ToolResult {
        id: String,
        content: String,
    },
    /// 需要用户确认权限。
    PermissionRequest(super::PermissionPrompt),
    /// 状态信息（用于 status line 显示）。
    Status(super::StatusInfo),
    /// Chat 完成。
    Done(ChatResult),
    /// Chat 出错。
    Error(String),
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
