//! Tools adapters 层共享生产常量（#1146 双轨归位）。

use std::time::Duration;

pub(crate) const SUB_AGENT_DEFAULT_TIMEOUT_SECS: u64 = 3600;
pub(crate) const SUB_AGENT_TIMEOUT_CAP_SECS: u64 = 10800;

pub(crate) const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

pub(crate) const TERM_GRACE: Duration = Duration::from_millis(200);

pub(crate) const BUILTIN_COMMIT_URI: &str = "aemeath-builtin://commit";

/// `BackgroundProcesss` logs action 缺省尾部/增量读取字节数（#252）。
pub(crate) const DEFAULT_BACKGROUND_LOG_MAX_BYTES: usize = 4096;
