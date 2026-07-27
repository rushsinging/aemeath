//! chat loop 的上下文类型定义。
//!
//! `SwitchClientFn` 和 `ChatLoopContext` 从 `loop_runner.rs` 拆出，
//! 降低主循环文件的体量。
//!
//! #1385: ChatLoopContext no longer duplicates service contracts (binding, tools,
//! policy, hooks, memory, reflection, reasoning, etc.).  All service-level state
//! lives in [`SessionRuntime`]; per-Run contracts come from [`RuntimeContext`]
//! assembled via `shell.runtime_context_factory.assemble()`.

use crate::application::main_loop::looping::events::ChatEventSink;
use crate::application::main_loop::looping::input_gate::InputEventDrainPort;
use crate::application::main_loop::looping::queue::QueueDrainPort;
use std::sync::Arc;

/// 模型切换构建器类型（#567）：接受 selection 字符串，async 返回
/// `(ProviderBinding, ModelSwitchResult)` 或 `String` 错误。
pub type SwitchClientFn = Arc<
    dyn Fn(
            &str,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<
                            (crate::ports::ProviderBinding, sdk::ModelSwitchResult),
                            String,
                        >,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

/// 单次 chat loop 的完整执行状态。
///
/// 由 `chat_impl()` 从 `RuntimeHandle` 构造，按值传入 `process_chat_loop()`，
/// 函数内解构消费。
///
/// #1385: Service contracts (policy, tools, hook, memory, task, reasoning, provider,
/// interaction, reflection) are now assembled per-run via
/// `shell.runtime_context_factory.assemble()`.  ChatLoopContext no longer
/// duplicates them.
///
/// Remaining fields are I/O channels, the session shell, initial user messages,
/// and per-session mutable bookkeeping.
#[allow(clippy::type_complexity)]
pub struct ChatLoopContext<S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    /// I/O channels
    pub sink: S,
    pub queue: Q,
    pub input_events: I,

    /// #1385: Session shell — single source for all session-level state.
    /// Non-Option; tests must construct a real `SessionRuntime` via the
    /// test helper or provide a minimal fixture.
    pub shell: crate::application::client::SessionRuntime,

    /// 本轮 chat loop 的初始消息（来自 user_input）。Runtime 不再持有/回写
    /// 会话链；历史由 Context backing 提供。
    pub initial_messages: Vec<share::message::Message>,

    /// Per-session read file tracking.
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,

    /// Task 7 debt: ChatLoopContext uses tools::SessionReminders type;
    /// shell holds share::memory::SessionReminders (different type).
    pub session_reminders: Arc<std::sync::Mutex<tools::SessionReminders>>,

    /// Session-scoped query port for idle commands — replaces four `Arc<Fn>`
    /// closures (#1385). Must stay in the Main Session shell; RuntimeContext
    /// must NOT reference this port.
    pub session_queries: Arc<dyn crate::ports::SessionQueryPort>,
}
