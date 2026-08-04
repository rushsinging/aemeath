//! Main session command driver 的启动输入。
//!
//! 该输入只承载 session actor 启动所需的 I/O、会话状态和本地 bookkeeping；
//! per-Run 活契约仍只由 `RunFactory::create()` 产生的 `RuntimeContext` 提供。

use crate::application::loop_engine::chat::events::ChatEventSink;
use crate::application::loop_engine::input_strategy::SessionInputPort;
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

/// Main session command driver 的一次启动输入。
///
/// Session actor 跨多个 Run 存活；每次真实用户输入都由它创建全新的
/// `RunInstance`，并交给共享 Loop Engine。该类型不复制 Runtime service，
/// 也不承担 per-Run capability 或执行状态所有权。
#[allow(clippy::type_complexity)]
pub struct SessionCommandDriverInput<S, I>
where
    S: ChatEventSink,
    I: SessionInputPort,
{
    pub sink: S,
    pub input_events: I,
    pub session: crate::application::client::SessionRuntime,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: Arc<std::sync::Mutex<tools::SessionReminders>>,
    pub session_queries: Arc<dyn crate::ports::SessionQueryPort>,
}
