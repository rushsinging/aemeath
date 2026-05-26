//! TUI 面向 SDK 的公共契约。
//!
//! 这些类型只描述 TUI 与 runtime 之间的稳定边界，不依赖具体 TUI
//! 渲染库，也不暴露 runtime 内部的 LLM client、tool registry、task store
//! 或取消 token。

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub type EventFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
pub type QueueFuture<'a> = Pin<Box<dyn Future<Output = Option<Vec<String>>> + Send + 'a>>;

/// runtime 返回给 TUI 的 chat handle。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatHandle {
    pub id: String,
}

/// TUI 从 runtime 接收 chat 流式事件的 sink。
pub trait ChatEventSink<Event>: Clone + Send + Sync + 'static {
    fn send_event<'a>(&'a self, event: Event) -> EventFuture<'a>;

    fn try_send_event(&self, event: Event);
}

/// runtime 请求 TUI drain 排队输入的端口。
pub trait QueueDrainPort: Clone + Send + Sync + 'static {
    fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a>;
}

/// TUI 可直接渲染的任务状态视图。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskStatusView {
    pub lines: Vec<String>,
}

/// SDK 级 TUI 启动上下文。
#[derive(Debug, Clone)]
pub struct TuiLaunchContext<MemoryConfig, Skill> {
    pub session_id: String,
    pub cwd: PathBuf,
    pub model_display: String,
    pub memory_config: MemoryConfig,
    pub skills_map: std::collections::HashMap<String, Skill>,
    pub initial_resume_id: Option<String>,
}
