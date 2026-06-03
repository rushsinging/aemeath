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
    invalid_transition_count: usize,
}

impl Default for ChatLoopFsm {
    fn default() -> Self {
        Self {
            state: ChatLoopState::Running,
            invalid_transition_count: 0,
        }
    }
}

impl ChatLoopState {
    fn apply(self, transition: ChatLoopTransition) -> Option<Self> {
        match (self, transition) {
            (_, ChatLoopTransition::StartTurn | ChatLoopTransition::ResumeRunning) => {
                Some(Self::Running)
            }
            (Self::Running, ChatLoopTransition::AwaitTool) => Some(Self::AwaitingTool),
            (
                Self::Running | Self::AwaitingTool | Self::Stopping | Self::StopHookBlocked,
                ChatLoopTransition::AwaitUser,
            ) => Some(Self::AwaitingUser),
            (Self::Running, ChatLoopTransition::Compact) => Some(Self::Compacting),
            (Self::Running | Self::AwaitingUser, ChatLoopTransition::TryStop) => {
                Some(Self::Stopping)
            }
            (Self::Stopping, ChatLoopTransition::StopBlocked) => Some(Self::StopHookBlocked),
            (Self::Stopping, ChatLoopTransition::StopSucceeded) => Some(Self::Done),
            (_, ChatLoopTransition::AbortCurrentLoop | ChatLoopTransition::CancelCurrentLoop) => {
                Some(Self::Done)
            }
            (_, _) => None,
        }
    }
}

impl ChatLoopFsm {
    pub fn state(&self) -> ChatLoopState {
        self.state
    }

    pub fn assert_state(&self, expected: ChatLoopState, context: &str) {
        if self.state != expected {
            log::warn!(
                "chat loop state guard failed: context={}, expected={:?}, actual={:?}",
                context,
                expected,
                self.state
            );
            debug_assert_eq!(self.state, expected, "{context}");
        }
    }

    pub fn invalid_transition_count(&self) -> usize {
        self.invalid_transition_count
    }

    pub fn transition(&mut self, transition: ChatLoopTransition) -> ChatLoopState {
        let previous = self.state;
        if let Some(next) = self.state.apply(transition) {
            self.state = next;
            log::debug!(
                "chat loop state transition: {:?} --{:?}--> {:?}",
                previous,
                transition,
                self.state
            );
        } else {
            self.invalid_transition_count += 1;
            log::warn!(
                "invalid chat loop state transition ignored: {:?} --{:?}-->",
                previous,
                transition
            );
        }
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
    fn test_chat_loop_state_assert_state_accepts_expected_state() {
        let fsm = ChatLoopFsm::default();

        fsm.assert_state(ChatLoopState::Running, "new fsm starts running");
    }

    #[test]
    #[should_panic(expected = "finalization guard")]
    fn test_chat_loop_state_assert_state_panics_in_debug_when_state_drifts() {
        let fsm = ChatLoopFsm::default();

        fsm.assert_state(ChatLoopState::Done, "finalization guard");
    }

    #[test]
    fn test_chat_loop_state_counts_invalid_transition_after_done() {
        let mut fsm = ChatLoopFsm::default();

        assert_eq!(
            fsm.transition(ChatLoopTransition::CancelCurrentLoop),
            ChatLoopState::Done
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::AwaitTool),
            ChatLoopState::Done
        );
        assert_eq!(fsm.invalid_transition_count(), 1);
    }
}
