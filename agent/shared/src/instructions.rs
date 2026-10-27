//! 指令文件加载通知契约（跨 BC 共享，#1863 W4）。
//!
//! 归属说明：该端口在语义上属于 Hook BC（它产生的正是 `InstructionsLoaded`
//! 订阅事件），但 Context 需要触发它、Context 又不得依赖 Hook crate，因此契约
//! 落在共享层：Hook 与 Context 各自依赖 share，双方都不引入跨 BC 依赖。
//! Hook 侧重新导出本 trait 作为其对外端口名。
//!
//! 端口刻意保持同步且无返回值——通知是 fire-and-forget，触发方（Context）
//! 不应等待 hook 子进程；异步派发由实现方（Runtime）自行调度。

/// 指令文件加载通知端口。
pub trait InstructionsLoadedNotifier: Send + Sync {
    /// 报告一个已加载的指令文件。
    ///
    /// `instruction_type` 取值由调用方约定（如 `"guidance"`、`"agents_md"`）。
    fn instructions_loaded(&self, file_path: &str, instruction_type: &str);
}
