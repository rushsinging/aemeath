//! 输入相关纯数据状态

use crate::tui::app::state::AskUserState;
use std::collections::VecDeque;

/// 输入框的所有可变数据（不含视图组件 InputArea）
#[derive(Default)]
pub(crate) struct InputState {
    pub just_pasted: bool,
    pub input_queue: VecDeque<String>,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    pub ask_user_reply_tx: Option<tokio::sync::oneshot::Sender<String>>,
    pub ask_user_state: Option<AskUserState>,
}

impl InputState {
    pub(crate) fn clear_queue(&mut self) {
        self.input_queue.clear();
    }

    pub(crate) fn push_queue(&mut self, input: String) -> usize {
        self.input_queue.push_back(input);
        self.input_queue.len()
    }

    pub(crate) fn drain_queue(&mut self) -> Vec<String> {
        self.input_queue.drain(..).collect()
    }

    pub(crate) fn queue_len(&self) -> usize {
        self.input_queue.len()
    }

    pub(crate) fn queue_preview(&self) -> String {
        self.input_queue
            .iter()
            .next()
            .map(|s| s.chars().take(40).collect())
            .unwrap_or_default()
    }
}
