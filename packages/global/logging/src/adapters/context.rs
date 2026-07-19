//! 诊断日志上下文。
//!
//! 执行字段通过不可变 [`LogContext`] task-local scope 传播；scope 外使用空快照。
//! `BOOT_TS`、`APP_VERSION` 与 `PID` 是进程级只读元数据，不承载执行上下文。

use crate::domain::{LogContext, LogContextPatch};
use std::future::Future;
use std::sync::OnceLock;

tokio::task_local! {
    static SCOPED_CONTEXT: LogContext;
}

/// 捕获当前 task scope 的不可变上下文；scope 外返回空快照。
pub fn capture() -> LogContext {
    scoped_context().unwrap_or_default()
}

pub(super) fn scoped_context() -> Option<LogContext> {
    SCOPED_CONTEXT.try_with(Clone::clone).ok()
}

/// 在由当前 context 和 patch 派生的 child scope 内执行 future。
pub async fn within<T>(patch: LogContextPatch, future: impl Future<Output = T>) -> T {
    let child = capture().patched(patch);
    SCOPED_CONTEXT.scope(child, future).await
}

/// 将已捕获的 context 显式绑定到尚未 spawn 的 future。
///
/// 正确用法是 `tokio::spawn(instrument(context, future))`。不得用它包裹已经创建的
/// `JoinHandle`；需要创建 task 时优先使用 [`spawn_instrumented`] 固定传播顺序。
pub async fn instrument<T>(context: LogContext, future: impl Future<Output = T>) -> T {
    SCOPED_CONTEXT.scope(context, future).await
}

/// 创建绑定了显式 context 的 Tokio task，避免先 spawn 后 instrument 的静默失效。
pub fn spawn_instrumented<T>(
    context: LogContext,
    future: impl Future<Output = T> + Send + 'static,
) -> tokio::task::JoinHandle<T>
where
    T: Send + 'static,
{
    tokio::spawn(instrument(context, future))
}

static BOOT_TS: OnceLock<String> = OnceLock::new();
static APP_VERSION: OnceLock<String> = OnceLock::new();
static PID: OnceLock<u32> = OnceLock::new();

/// 设置进程启动时间戳（本地时间 RFC3339）。`init_logging` 时调用一次。
pub fn set_boot_ts(ts: String) {
    let _ = BOOT_TS.set(ts);
}

/// 设置 app 版本号。`init_logging` 时调用一次。
pub fn set_app_version(ver: String) {
    let _ = APP_VERSION.set(ver);
}

pub fn boot_ts() -> Option<&'static str> {
    BOOT_TS.get().map(|s| s.as_str())
}

/// 获取进程 pid。惰性取 `std::process::id()`。
pub fn pid() -> u32 {
    *PID.get_or_init(std::process::id)
}

pub fn app_version() -> Option<&'static str> {
    APP_VERSION.get().map(|s| s.as_str())
}

#[cfg(test)]
#[path = "context_scope_tests.rs"]
mod scope_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_returns_process_id() {
        let p = pid();
        assert_eq!(p, std::process::id());
    }
}
