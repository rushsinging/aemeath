//! Tools adapters 层共享生产常量（#1146 双轨归位）。

use std::time::Duration;

pub(crate) const SUB_AGENT_DEFAULT_TIMEOUT_SECS: u64 = 3600;
pub(crate) const SUB_AGENT_TIMEOUT_CAP_SECS: u64 = 10800;

pub(crate) const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

pub(crate) const TERM_GRACE: Duration = Duration::from_millis(200);

pub(crate) const BUILTIN_COMMIT_URI: &str = "aemeath-builtin://commit";

/// `BackgroundProcesss` logs action 缺省尾部/增量读取字节数（#252）。
pub(crate) const DEFAULT_BACKGROUND_LOG_MAX_BYTES: usize = 4096;

/// Grep 索引模式文本的字符预算上限：保证索引结果落在通用落盘阈值最严档
/// （`ToolResultPolicy::scaled_for_context_window` 的下限）之内，
/// 使「哪里命中」的发现性信息始终完整可见、不被落盘预览截断。
/// 该值需与下限保持同量级：下限调整时必须同步复核，不得超出。
pub(crate) const GREP_INDEX_TEXT_BUDGET_CHARS: usize = 2_000;

/// 索引文本为汇总 header 与收窄提示预留的字符空间，
/// 避免截断后总长超出 [`GREP_INDEX_TEXT_BUDGET_CHARS`]。
pub(crate) const GREP_INDEX_HEADER_RESERVE_CHARS: usize = 260;
