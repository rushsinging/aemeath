//! InputBuffer — 入站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2 / §3。
//! 细化由 #874 负责。

use crate::business::loop_engine::LoopInput;

/// 入站缓冲端口——Runtime loop 从此端口 drain 用户输入。
///
/// Main Run = TUI 通道 + 忙期 buffer（追问排队）。
/// Sub Run = FixedQueue（固定初始 prompt 队列）。
pub trait InputBuffer: Send + Sync {
    /// 取出所有待处理的输入。
    fn drain(&self) -> Vec<LoopInput>;
}
