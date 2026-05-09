//! Task reminder passive injection.
//!
//! Automatically injects a concise task status reminder as a `<system-reminder>`
//! user message every N turns. This lets the LLM perceive progress without
//! actively calling `TaskList`.

use aemeath_core::message::{ContentBlock, Message, Role};
use aemeath_core::task::TaskStore;

/// How many turns since last TaskCreate/TaskUpdate before injecting a reminder.
const TURNS_SINCE_WRITE: u64 = 5;
/// Minimum gap between consecutive reminders.
const TURNS_BETWEEN_REMINDERS: u64 = 5;

/// Tracks injection throttling for task reminders.
#[derive(Debug, Clone)]
pub struct TaskReminderState {
    /// Turn number when a TaskCreate or TaskUpdate tool was last called by the assistant.
    last_task_management_turn: u64,
    /// Turn number when we last injected a reminder.
    last_reminder_turn: u64,
}

impl TaskReminderState {
    pub fn new() -> Self {
        Self {
            last_task_management_turn: 0,
            last_reminder_turn: 0,
        }
    }

    /// Check whether a TaskCreate / TaskUpdate tool use appears in the given messages.
    /// Scans the most recent assistant message's tool_use blocks.
    pub fn update_from_messages(&mut self, current_turn: u64, messages: &[Message]) {
        for msg in messages.iter().rev() {
            if msg.role == Role::Assistant {
                // Scan tool_use blocks in this assistant message
                for block in &msg.content {
                    if let ContentBlock::ToolUse { name, .. } = block {
                        match name.as_str() {
                            "TaskCreate" | "TaskUpdate" => {
                                self.last_task_management_turn = current_turn;
                                return;
                            }
                            _ => {}
                        }
                    }
                }
                // Only scan the most recent assistant message
                return;
            }
        }
    }

    /// Try to build a task reminder for the current turn.
    ///
    /// Returns `Some(Message)` if all conditions are met:
    /// 1. Enough turns since last TaskCreate/TaskUpdate
    /// 2. Enough turns since last reminder
    /// 3. Task list is not empty
    ///
    /// Returns `None` if throttled or tasks are empty.
    pub async fn build_reminder(
        &mut self,
        current_turn: u64,
        task_store: &TaskStore,
    ) -> Option<Message> {
        // Throttle: must have ≥ TURNS_SINCE_WRITE since last task management
        if current_turn < self.last_task_management_turn + TURNS_SINCE_WRITE {
            return None;
        }
        // Throttle: must have ≥ TURNS_BETWEEN_REMINDERS since last reminder
        if current_turn < self.last_reminder_turn + TURNS_BETWEEN_REMINDERS {
            return None;
        }

        let stats = task_store.stats().await;
        // Fuse: no tasks, no reminder
        if stats.total == 0 {
            return None;
        }

        self.last_reminder_turn = current_turn;

        let text = format!(
            "Task progress: {completed}/{total} done, {in_progress} active, {pending} pending. Use TaskList to see details.",
            completed = stats.completed,
            total = stats.total,
            in_progress = stats.in_progress,
            pending = stats.pending,
        );

        let reminder = format!("<system-reminder>\n{}\n</system-reminder>", text);

        Some(Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: reminder }],
        })
    }
}

impl Default for TaskReminderState {
    fn default() -> Self {
        Self::new()
    }
}
