//! 纯值常量（#1146 placement 归位）。

pub(crate) const TOOL_LARGE_FIELD_PREVIEW_LIMIT: usize = 256;

pub(super) const TOOL_STREAM_PREVIEW_LIMIT: usize = 512;

pub(super) const TOOL_RESULT_PREVIEW_LIMIT: usize = 16 * 1024;

/// ToolSearch 结构化结果逐条 `description` 的保留字符上限（#1957）。
///
/// 描述只供 TUI 摘要展示，超出部分带省略标记截断，**NEVER** 因此丢失
/// `name` / `kind` / `is_read_only` 等结构化字段。
pub(crate) const TOOL_SEARCH_DESCRIPTION_BUDGET_CHARS: usize = 256;

/// ToolSearch 结构化结果在 TUI 预览限额内保留的条目数上限（#1957）。
///
/// 超出部分从尾部丢弃并在结果对象上保留 `tools_truncated: "total/omitted"`
/// 标记，保证结果始终是合法 JSON 对象。
pub(crate) const TOOL_SEARCH_ITEM_BUDGET: usize = 40;

/// 收敛 ToolSearch 结果时为 `tools_truncated` 标记预留的字节余量（#1957）。
pub(crate) const STRUCTURED_MARKER_RESERVE_BYTES: usize = 64;

/// TUI 预览省略标记的固定后缀（幂等判定用，NEVER 二次截断已标记文本）。
pub(crate) const TUI_OMISSION_SUFFIX_MARKER: &str = "chars omitted from TUI preview)";
