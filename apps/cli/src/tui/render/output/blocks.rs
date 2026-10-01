//! 顶层 block 级渲染组件。每个组件 fn(view, ctx) -> RenderedBlock。

pub mod ask_user;
pub mod assistant_message;
mod constants;
pub mod diagnostic;
pub mod edit_diff;
pub mod hook_notice;
pub mod thinking;
pub mod tool_call;
pub mod tool_result;
pub mod user_message;

#[cfg(test)]
#[path = "blocks_tests.rs"]
mod tests;
