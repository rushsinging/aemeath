//! AgentClient trait — Agent Runtime 对外的统一接口。

use async_trait::async_trait;

use crate::{ChatRequest, ChatStream};

/// Agent Runtime 的统一客户端 trait。
///
/// #567 后 trait 只有 `chat()`——所有交互通过事件流：
/// - **写操作** → `ChatInputEvent`（push_input_event → gate → loop idle 分支执行）
/// - **结果回传** → `ChatEvent` 流（事件驱动，TUI 监听）
#[async_trait]
pub trait AgentClient: Send + Sync + 'static {
    /// 发起一次 Chat，返回事件流。
    ///
    /// TUI 通过 `ChatRequest.input_events` 发送 `ChatInputEvent`，
    /// 通过 `ChatStream`（`ChatEvent` 流）接收结果。
    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, super::SdkError>;
}
