use super::chat_turn::ChatTurn;
use super::ids::{ChatId, ChatTurnId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chat {
    pub id: ChatId,
    pub user_submission: String,
    pub status: ChatStatus,
    pub turns: Vec<ChatTurn>,
}

impl Chat {
    pub fn new(id: ChatId, user_submission: String) -> Self {
        Self {
            id,
            user_submission,
            status: ChatStatus::Running,
            turns: vec![ChatTurn::new(ChatTurnId::new_v7(), 0)],
        }
    }

    pub fn active_turn_mut(&mut self) -> Option<&mut ChatTurn> {
        self.turns.last_mut()
    }

    pub fn active_turn(&self) -> Option<&ChatTurn> {
        self.turns.last()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatStatus {
    Created,
    Running,
    Completing,
    Completed,
    Failed,
    Cancelled,
}
