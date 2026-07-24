//! InputBuffer — 入站端口契约。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2 / §3。
//! 具体实现见 `adapters/input_buffer.rs`。

use crate::application::loop_engine::LoopInput;

/// 入站缓冲端口——Runtime loop 从此端口 drain 用户输入。
///
/// Main Run = TUI 通道 + 忙期 buffer（追问排队）。
/// Sub Run = FixedQueue（固定初始 prompt 队列）。
pub trait InputBuffer: Send + Sync {
    /// 取出所有待处理的输入。
    fn drain(&self) -> Vec<LoopInput>;
}
