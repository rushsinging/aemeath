//! input 域纯值常量（#1146 placement 归位）。

/// OSC 52 payload 上限（base64 编码后字节数）：主流终端对超长序列会静默丢弃或截断。
pub(crate) const OSC52_MAX_ENCODED_BYTES: usize = 100_000;
