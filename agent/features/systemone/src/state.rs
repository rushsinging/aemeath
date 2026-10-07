//! 进程级状态容器（`pattern.all.constant-placement`：状态容器归位 `state.rs`，#1146）。

/// 模型安装暂存目录唯一序号：进程内原子递增，
/// 与 pid 共同构成 `<root>/.tmp-<revision>-<pid>-<seq>` 的唯一暂存名。
pub(crate) static STAGING_SEQUENCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
