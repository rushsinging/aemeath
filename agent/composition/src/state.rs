//! 状态容器（#1146 placement 归位）。

use std::sync::{Mutex, OnceLock};

/// Logging 初始化串行化锁（app.rs 唯一消费）。
pub(crate) static LOGGING_INIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
