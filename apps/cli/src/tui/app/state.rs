//! TUI 纯数据状态层
//!
//! 遵循 TEA 架构：所有 State 都是纯数据，不含视图组件，不含副作用引用。

pub(crate) mod ask_user;
pub(crate) mod chat;
pub(crate) mod input;
pub(crate) mod layout;
pub(crate) mod session;

pub(crate) use ask_user::{AskUserState, BUILTIN_OPTION_ALL, BUILTIN_OPTION_CHAT, BUILTIN_OPTION_NONE};
pub(crate) use chat::ChatState;
pub(crate) use input::InputState;
pub(crate) use layout::UiLayout;
pub(crate) use session::SessionState;

/// 终端尺寸快照
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalSize {
    pub width: u16,
    pub height: u16,
}

#[cfg(test)]
mod tests;
