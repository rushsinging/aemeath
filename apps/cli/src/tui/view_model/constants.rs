//! 纯值常量（#1146 placement 归位）。

/// 最大嵌套深度：top(0) → tool_call(1) → result-content(2)。深度从 0 计，最深合法子层级为 2。
/// 最大嵌套深度：top(0) → tool_call(1) → result-content(2)。深度从 0 计，最深合法子层级为 2。
pub const MAX_BLOCK_DEPTH: usize = 3;
