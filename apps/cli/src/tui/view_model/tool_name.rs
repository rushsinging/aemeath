//! Tool display name 映射（内部名 → 用户可见名）。
//!
//! 位于 view_model 层，render 和 view_assembler 均可引用，
//! 避免 view_assembler 反向依赖 render 层。

use super::state::TOOL_DISPLAY_NAMES;

/// 返回工具的用户可见 display name。未注册的工具原样返回内部名。
pub fn tool_display_name(name: &str) -> &str {
    TOOL_DISPLAY_NAMES.get(name).copied().unwrap_or(name)
}

#[cfg(test)]
#[path = "tool_name_tests.rs"]
mod tests;
