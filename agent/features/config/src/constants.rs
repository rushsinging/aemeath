//! crate 身份常量（#1146 轨道 B 归位：自 `lib.rs` 抽出，lib.rs 仅 re-export，
//! crate 内 `crate::LOG_TARGET` 引用路径不变）。

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:config";
