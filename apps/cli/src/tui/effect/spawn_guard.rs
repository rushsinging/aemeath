//! 后台 tokio task 的统一 panic 兜底。
//! tokio 默认会静默吞掉 spawned task 的 panic（仅 panic hook 留痕）；
//! 此 helper 在 future 外层加 catch_unwind，将 panic 转为可见错误日志。

use futures::FutureExt;

/// spawn 一个带 panic 兜底的后台进程。task 内 panic 不会传播，只记录 error 日志。
pub fn spawn_guarded<F>(label: &'static str, fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    composition::delivery_logging::spawn_instrumented(
        composition::delivery_logging::capture(),
        async move {
            if let Err(panic) = std::panic::AssertUnwindSafe(fut).catch_unwind().await {
                let msg = crate::panic_hook::payload_message(panic.as_ref());
                crate::tui::log_error!("后台进程 {} panic: {}", label, msg);
            }
        },
    );
}

#[cfg(test)]
#[path = "spawn_guard_tests.rs"]
mod tests;
