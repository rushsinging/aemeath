//! Tool display name 映射（内部名 → 用户可见名）。
//!
//! 位于 view_model 层，render 和 view_assembler 均可引用，
//! 避免 view_assembler 反向依赖 render 层。
//!
//! #1964 Task 3：唯一真相源下沉至 `sdk::tool_display_name`，
//! runtime（转后台摘要）与 TUI 共用同一张表；本模块只做薄委托，
//! **NEVER** 再在此放字面量映射表。

/// 返回工具的用户可见 display name。未注册的工具原样返回内部名。
pub fn tool_display_name(name: &str) -> &str {
    sdk::tool_display_name(name)
}

#[cfg(test)]
#[path = "tool_name_tests.rs"]
mod tests;
