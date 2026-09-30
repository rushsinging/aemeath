//! bash 工具常量（#1146 双轨归位）。

pub(crate) const MAX_CAPTURE_BYTES: usize = 10 * 1024 * 1024; // 10 MB
pub(crate) const MAX_STREAM_LINE_BYTES: usize = 16 * 1024;

pub(crate) const PREVIEW_MAX: usize = 512;

pub(crate) const READER_DRAIN_TIMEOUT_MS: u64 = 500;

pub(crate) const CWD_MARKER: &str = "__AEMEATH_CWD__=";
