use super::chat_turn::ChatRun;
use super::ids::{ChatId, ChatRunId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chat {
    pub id: ChatId,
    pub user_submission: String,
    pub status: ChatStatus,
    pub runs: Vec<ChatRun>,
}

impl Chat {
    pub fn new(id: ChatId, user_submission: String) -> Self {
        Self {
            id,
            user_submission,
            status: ChatStatus::Running,
            runs: vec![ChatRun::new(ChatRunId::new_v7(), 0)],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatStatus {
    Running,
    Completing,
}
