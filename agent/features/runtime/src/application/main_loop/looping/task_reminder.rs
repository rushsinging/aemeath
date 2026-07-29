//! Task reminder state used while freezing ContextRequest input.

use share::message::{ContentBlock, Role};

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

    pub fn update_from_messages<'a>(
        &mut self,
        current_turn: u64,
        messages: impl IntoIterator<Item = &'a share::message::Message>,
    ) {
        let messages = messages.into_iter().collect::<Vec<_>>();
        for message in messages.into_iter().rev() {
            if message.role != Role::Assistant {
                continue;
            }
            if message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::ToolUse { name, .. }
                        if matches!(
                            name.as_str(),
                            "TaskCreate"
                                | "TaskUpdate"
                                | "TaskBlockBy"
                                | "TaskStop"
                                | "TaskListCreate"
                                | "TaskListComplete"
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
