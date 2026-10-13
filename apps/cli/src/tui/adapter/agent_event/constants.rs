//! 纯值常量（#1146 placement 归位）。

pub(crate) const TOOL_LARGE_FIELD_PREVIEW_LIMIT: usize = 256;

pub(super) const TOOL_STREAM_PREVIEW_LIMIT: usize = 512;

/// TUI tool_result 展示载荷上限（#1444 有界预览；#1957 由 16KB 提升到 64KB）。
///
/// 仍是**有界**预览：防止 GB 级工具结果在物化成 document 前把内存打爆；
/// 提升后常见的大结果（如 95 条 ToolSearch ≈24KB）不再触发任何裁剪。
pub(super) const TOOL_RESULT_PREVIEW_LIMIT: usize = 64 * 1024;

/// ToolSearch 结构化结果逐条 `description` 的保留字符上限（#1957）。
///
/// 描述只供 TUI 摘要展示，超出部分带省略标记截断，**NEVER** 因此丢失
/// `name` / `kind` / `is_read_only` 等结构化字段。
pub(crate) const TOOL_SEARCH_DESCRIPTION_BUDGET_CHARS: usize = 256;

/// 收敛 ToolSearch 结果时为 `tools_truncated` 标记预留的字节余量（#1957）。
pub(crate) const STRUCTURED_MARKER_RESERVE_BYTES: usize = 64;

/// TUI 预览省略标记的固定后缀（幂等判定用，NEVER 二次截断已标记文本）。
pub(crate) const TUI_OMISSION_SUFFIX_MARKER: &str = "chars omitted from TUI preview)";
