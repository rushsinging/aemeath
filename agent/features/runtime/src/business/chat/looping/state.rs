#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatLoopState {
    Running,
    AwaitingTool,
    AwaitingUser,
    Compacting,
    Stopping,
    StopHookBlocked,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatLoopTransition {
    StartTurn,
    AwaitTool,
    AwaitUser,
    Compact,
    TryStop,
    StopBlocked,
    StopSucceeded,
    ResumeRunning,
    AbortCurrentLoop,
    CancelCurrentLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatLoopFsm {
    state: ChatLoopState,
}

impl Default for ChatLoopFsm {
    fn default() -> Self {
        Self {
            state: ChatLoopState::Running,
        }
    }
}

impl ChatLoopState {
    fn apply(self, transition: ChatLoopTransition) -> Self {
        match (self, transition) {
            (_, ChatLoopTransition::StartTurn | ChatLoopTransition::ResumeRunning) => Self::Running,
            (Self::Running, ChatLoopTransition::AwaitTool) => Self::AwaitingTool,
            (
                Self::Running | Self::AwaitingTool | Self::Stopping | Self::StopHookBlocked,
                ChatLoopTransition::AwaitUser,
            ) => Self::AwaitingUser,
            (Self::Running, ChatLoopTransition::Compact) => Self::Compacting,
            (Self::Running | Self::AwaitingUser, ChatLoopTransition::TryStop) => Self::Stopping,
            (Self::Stopping, ChatLoopTransition::StopBlocked) => Self::StopHookBlocked,
            (Self::Stopping, ChatLoopTransition::StopSucceeded) => Self::Done,
            (_, ChatLoopTransition::AbortCurrentLoop | ChatLoopTransition::CancelCurrentLoop) => {
                Self::Done
            }
            (state, _) => state,
        }
    }
}

impl ChatLoopFsm {
    pub fn state(&self) -> ChatLoopState {
        self.state
    }

    pub fn transition(&mut self, transition: ChatLoopTransition) -> ChatLoopState {
        let previous = self.state;
        self.state = self.state.apply(transition);
        log::debug!(
            "chat loop state transition: {:?} --{:?}--> {:?}",
            previous,
            transition,
            self.state
        );
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_loop_state_stop_hook_blocked_must_resume_before_done() {
        let mut fsm = ChatLoopFsm::default();

        assert_eq!(fsm.state(), ChatLoopState::Running);
        assert_eq!(
            fsm.transition(ChatLoopTransition::TryStop),
            ChatLoopState::Stopping
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::StopBlocked),
            ChatLoopState::StopHookBlocked
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::ResumeRunning),
            ChatLoopState::Running
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::TryStop),
            ChatLoopState::Stopping
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::StopSucceeded),
            ChatLoopState::Done
        );
    }

    #[test]
    fn test_chat_loop_state_tool_and_user_boundaries_resume_running() {
        let mut fsm = ChatLoopFsm::default();

        assert_eq!(
            fsm.transition(ChatLoopTransition::AwaitTool),
            ChatLoopState::AwaitingTool
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::AwaitUser),
            ChatLoopState::AwaitingUser
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::ResumeRunning),
            ChatLoopState::Running
        );
    }

    #[test]
    fn test_chat_loop_state_abort_and_cancel_enter_done_from_any_state() {
        let mut aborting = ChatLoopFsm::default();
        assert_eq!(
            aborting.transition(ChatLoopTransition::AwaitTool),
            ChatLoopState::AwaitingTool
        );
        assert_eq!(
            aborting.transition(ChatLoopTransition::AbortCurrentLoop),
            ChatLoopState::Done
        );

        let mut cancelling = ChatLoopFsm::default();
        assert_eq!(
            cancelling.transition(ChatLoopTransition::TryStop),
            ChatLoopState::Stopping
        );
        assert_eq!(
            cancelling.transition(ChatLoopTransition::CancelCurrentLoop),
            ChatLoopState::Done
        );
    }
}
