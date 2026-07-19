//! Task reminder state used while freezing ContextRequest input.

use share::message::{ContentBlock, Message, Role};

/// Tracks the most recent Task tool activity. Context owns final reminder text
/// placement; Runtime only observes whether task management occurred.
#[derive(Debug, Clone, Default)]
pub struct TaskReminderState {
    last_task_management_turn: u64,
}

impl TaskReminderState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_from_messages(&mut self, current_turn: u64, messages: &[Message]) {
        for message in messages.iter().rev() {
            if message.role != Role::Assistant {
                continue;
            }
            if message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::ToolUse { name, .. }
                        if matches!(
                            name.as_str(),
                            "TaskCreate" | "TaskUpdate" | "TaskListCreate" | "TaskListComplete"
                        )
                )
            }) {
                self.last_task_management_turn = current_turn;
            }
            break;
        }
    }

    #[cfg(test)]
    pub const fn last_task_management_turn(&self) -> u64 {
        self.last_task_management_turn
    }
}

#[cfg(test)]
#[path = "task_reminder_tests.rs"]
mod tests;
