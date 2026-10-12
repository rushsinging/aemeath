//! memory 工具常量（#1146 双轨归位）。

pub(super) const MAX_CONTENT_CHARS: usize = 500;
pub(super) const MAX_TAGS: usize = 10;
pub(super) const MAX_TAG_CHARS: usize = 32;

/// MemoryList 默认每页条数。
pub(super) const MEMORY_LIST_DEFAULT_LIMIT: usize = 20;
/// MemoryList / Search 单次上限。
pub(super) const MEMORY_PAGE_MAX_LIMIT: usize = 50;
/// LLM text 中每条 content 预览宽度（字符）。
pub(super) const MEMORY_RESULT_CONTENT_PREVIEW_CHARS: usize = 80;
