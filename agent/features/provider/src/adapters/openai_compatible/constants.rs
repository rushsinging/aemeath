//! openai_compatible 子域常量（#1146 双轨归位）。

pub(crate) const PREVIEW_CHARS: usize = 60;
pub(crate) const MIN_OVERLAP_LEN: usize = 3;
/// 流停滞检测阈值（单一真相源：`business::STALL_THRESHOLD_SECS`）
/// 流停滞检测阈值（单一真相源：`business::STALL_THRESHOLD_SECS`）
pub(crate) const STALL_THRESHOLD: std::time::Duration =
    std::time::Duration::from_secs(crate::STALL_THRESHOLD_SECS);

/// 流空闲超时（单一真相源：`business::OPENAI_STREAM_IDLE_TIMEOUT_SECS`）
/// 流空闲超时（单一真相源：`business::OPENAI_STREAM_IDLE_TIMEOUT_SECS`）
pub(crate) const STREAM_IDLE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(crate::OPENAI_STREAM_IDLE_TIMEOUT_SECS);
