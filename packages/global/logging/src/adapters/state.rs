//! 状态容器（#1146 placement 归位）。

use std::sync::atomic::AtomicUsize;
use std::sync::OnceLock;

use super::file_sink::UnifiedLogger;
use super::native_stderr::SavedStderr;

pub(crate) static BOOT_TS: OnceLock<String> = OnceLock::new();
pub(crate) static APP_VERSION: OnceLock<String> = OnceLock::new();
pub(crate) static PID: OnceLock<u32> = OnceLock::new();

pub(crate) static UNKNOWN_TARGET_REPORTS: AtomicUsize = AtomicUsize::new(0);

pub(crate) static LOGGER: OnceLock<&'static UnifiedLogger> = OnceLock::new();

/// 进程级保存位：stderr 只路由一次，副本随首次路由写入。
pub(crate) static SAVED_STDERR: OnceLock<SavedStderr> = OnceLock::new();
