//! Tools crate 身份常量（#1146 双轨归位）。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:tools";

/// ToolSearch 语义重排候选文案字符上限（与 memory 重排同口径，prose 评分头截断防爆 token）。
pub(crate) const TOOL_SEARCH_CRITERIA_MAX_CHARS: usize = 500;

/// ToolSearch 语义重排 instructions（Qwen3-Reranker instruct 定向口径）。
pub(crate) const TOOL_SEARCH_INSTRUCTIONS: &str =
    "Given a tool search query from a coding agent, select the most relevant tools.";
