//! 跨 crate 边界错误分类标准（façade 边界收敛 SOP 的一部分）。
//!
//! 每个 feature crate 的边界错误（如 `AuditError`）使用与 [`ErrorCategory`]
//! 对齐的粗分类变体；细粒度内部错误（`pub(crate)`）在边界经 `From`
//! 折叠为分类 + 消息，永不越界。

/// 边界错误粗分类：所有 crate 的边界错误变体与之对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// 底层存储/IO 失败。
    Storage,
    /// 调用方输入或状态非法。
    Invalid,
    /// 依赖不可用（worker 停止、锁占用、未接线等）。
    Unavailable,
}

impl ErrorCategory {
    /// 统一的粗分类名（日志与边界错误的稳定标签）。
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCategory::Storage => "storage",
            ErrorCategory::Invalid => "invalid",
            ErrorCategory::Unavailable => "unavailable",
        }
    }
}
