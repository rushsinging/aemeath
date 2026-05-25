//! 输入相关纯数据状态

use crate::tui::state::AskUserState;

/// 输入框的所有可变数据（不含视图组件 InputArea）
pub(crate) struct InputState {
    pub just_pasted: bool,
    pub input_queue: std::collections::VecDeque<String>,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    pub ask_user_reply_tx: Option<tokio::sync::oneshot::Sender<String>>,
    pub ask_user_state: Option<AskUserState>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            just_pasted: false,
            input_queue: std::collections::VecDeque::new(),
            last_click: None,
            ask_user_reply_tx: None,
            ask_user_state: None,
        }
    }
}
