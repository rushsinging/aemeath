//! Crate 身份常量（#1146 组3a 常量归位）。

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量.
pub(crate) const LOG_TARGET: &str = "aemeath:agent:runtime";
