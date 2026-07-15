//! InputBuffer — 入站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2 / §3。
//! 细化由 #874 负责。

use crate::application::loop_engine::LoopInput;
use std::sync::Arc;

#[derive(Clone, Default)]
pub(crate) struct RuntimeQueueDrainPort {
    inner: Option<Arc<dyn sdk::QueueDrainPort>>,
}

impl RuntimeQueueDrainPort {
    pub(crate) fn new(inner: Option<Arc<dyn sdk::QueueDrainPort>>) -> Self {
        Self { inner }
    }
}

impl crate::application::chat::QueueDrainPort for RuntimeQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> crate::application::chat::QueueFuture<'a> {
        Box::pin(async move {
            match &self.inner {
                Some(inner) => inner.drain_queued_input().await,
                None => None,
            }
        })
    }
}

#[derive(Clone, Default)]
pub(crate) struct RuntimeInputEventDrainPort {
    inner: Option<Arc<dyn sdk::ChatInputEventPort>>,
}

impl RuntimeInputEventDrainPort {
    pub(crate) fn new(inner: Option<Arc<dyn sdk::ChatInputEventPort>>) -> Self {
        Self { inner }
    }
}

impl crate::application::chat::InputEventDrainPort for RuntimeInputEventDrainPort {
    fn drain_input_events<'a>(&'a self) -> crate::application::chat::InputEventFuture<'a> {
        Box::pin(async move {
            match &self.inner {
                Some(inner) => inner.drain_input_events().await,
                None => Vec::new(),
            }
        })
    }

    fn recv_next_input<'a>(&'a self) -> crate::application::chat::InputEventOptFuture<'a> {
        Box::pin(async move {
            match &self.inner {
                Some(port) => port.recv_next().await,
                None => None,
            }
        })
    }
}

/// 入站缓冲端口——Runtime loop 从此端口 drain 用户输入。
///
/// Main Run = TUI 通道 + 忙期 buffer（追问排队）。
/// Sub Run = FixedQueue（固定初始 prompt 队列）。
pub trait InputBuffer: Send + Sync {
    /// 取出所有待处理的输入。
    fn drain(&self) -> Vec<LoopInput>;
}
