//! Task reminder passive injection.
//!
//! Automatically injects a concise task status reminder as a `<system-reminder>`
//! user message every N turns. This lets the LLM perceive progress without
//! actively calling `TaskList`.

use share::message::{ContentBlock, Message, Role};
use storage::api::{TaskStatus, TaskStore};

/// How many turns since last TaskCreate/TaskUpdate before injecting the FIRST reminder.
/// Shorter than TURNS_SINCE_WRITE so early follow-ups trigger a reminder sooner.
const TURNS_SINCE_WRITE_FIRST: u64 = 3;
/// How many turns since last TaskCreate/TaskUpdate before injecting a subsequent reminder.
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
                            "TaskCreate" | "TaskUpdate" | "TaskListCreate" | "TaskListComplete" => {
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
        let is_first_reminder = self.last_reminder_turn == 0;

        // Throttle: must have enough turns since last task management.
        // First reminder uses a shorter threshold (TURNS_SINCE_WRITE_FIRST) so
        // early follow-ups trigger a reminder sooner; subsequent reminders use
        // the longer TURNS_SINCE_WRITE.
        let write_threshold = if is_first_reminder {
            TURNS_SINCE_WRITE_FIRST
        } else {
            TURNS_SINCE_WRITE
        };
        if current_turn < self.last_task_management_turn + write_threshold {
            return None;
        }
        // Throttle: must have ≥ TURNS_BETWEEN_REMINDERS since last reminder
        // (skip on first reminder — no previous reminder to space from)
        if !is_first_reminder && current_turn < self.last_reminder_turn + TURNS_BETWEEN_REMINDERS {
            return None;
        }

        let mut lines = Vec::new();
        let mut pending_batches = task_store.lists_with_pending().await;
        if pending_batches.is_empty() {
            let current_batch = task_store.current_batch().await;
            let tasks = task_store
                .tasks_in_batch(
                    current_batch,
                    &[TaskStatus::Pending, TaskStatus::InProgress],
                )
                .await;
            if !tasks.is_empty() {
                pending_batches.push(task_store.get_or_create_batch(current_batch).await);
            }
        }

        for batch in pending_batches {
            let tasks = task_store
                .tasks_in_batch(batch.id, &[TaskStatus::Pending, TaskStatus::InProgress])
                .await;
            if tasks.is_empty() {
                continue;
            }
            let task_text = tasks
                .iter()
                .map(|task| format!("#{} {} [{:?}]", task.id, task.subject, task.status))
                .collect::<Vec<_>>()
                .join(", ");
            let summary = batch.summary.as_deref().unwrap_or("no summary");
            lines.push(format!(
                "Task batch #{} [{:?}] — summary: {} — {}",
                batch.id, batch.status, summary, task_text
            ));
        }
        if lines.is_empty() {
            return None;
        }

        self.last_reminder_turn = current_turn;

        let text = format!(
            "Task reminders are grouped by task batch and may belong to earlier user requests. If unrelated to the latest user message, answer the latest user message first.\n{}\nUse TaskList only when the user asks to continue/resume or when a listed task is clearly relevant.",
            lines.join("\n")
        );
        let reminder = format!("<system-reminder>\n{}\n</system-reminder>", text);

        Some(Message::system_generated_user(reminder))
    }
}

impl Default for TaskReminderState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::api::TaskStatus;

    fn reminder_text(message: &Message) -> String {
        message.text_content()
    }

    #[tokio::test]
    async fn test_build_reminder_first_throttled_before_threshold() {
        let store = TaskStore::new();
        store
            .create("遗留任务".to_string(), "old request".to_string(), None)
            .await;

        let mut state = TaskReminderState::new();
        // Turn 2 < TURNS_SINCE_WRITE_FIRST(3) → should NOT trigger
        let reminder = state.build_reminder(2, &store).await;
        assert!(reminder.is_none());
    }

    #[tokio::test]
    async fn test_build_reminder_first_triggers_at_threshold() {
        let store = TaskStore::new();
        store
            .create("遗留任务".to_string(), "old request".to_string(), None)
            .await;

        let mut state = TaskReminderState::new();
        // Turn 3 == TURNS_SINCE_WRITE_FIRST(3) → should trigger
        let reminder = state
            .build_reminder(TURNS_SINCE_WRITE_FIRST, &store)
            .await
            .expect("reminder exists");
        let text = reminder_text(&reminder);
        assert!(text.contains("Task batch"));
    }

    #[tokio::test]
    async fn test_build_reminder_groups_by_batch_and_warns_unrelated() {
        let store = TaskStore::new();
        store
            .create_list("旧任务".to_string(), "修复 task 状态".to_string())
            .await;
        let task = store
            .create("分析旧任务".to_string(), "修复 task 状态".to_string(), None)
            .await;
        let mut state = TaskReminderState::new();
        let reminder = state
            .build_reminder(TURNS_SINCE_WRITE_FIRST, &store)
            .await
            .expect("reminder exists");
        let text = reminder_text(&reminder);

        assert!(text.contains("Task batch #0"));
        assert!(text.contains("修复 task 状态"));
        assert!(text.contains("may belong to earlier user requests"));
        assert!(text.contains("If unrelated to the latest user message"));
        assert_eq!(
            store.get(&task.id).await.unwrap().status,
            TaskStatus::Pending
        );
    }

    #[tokio::test]
    async fn test_build_reminder_omits_all_completed_batches() {
        let store = TaskStore::new();
        let task = store
            .create("完成任务".to_string(), "done".to_string(), None)
            .await;
        store
            .update(&task.id, |task| task.status = TaskStatus::Completed)
            .await;

        let mut state = TaskReminderState::new();
        let reminder = state
            .build_reminder(TURNS_SINCE_WRITE_FIRST, &store)
            .await;

        assert!(reminder.is_none());
    }

    #[tokio::test]
    async fn test_build_reminder_mentions_use_tasklist_only_when_relevant() {
        let store = TaskStore::new();
        store
            .create("遗留任务".to_string(), "old request".to_string(), None)
            .await;

        let mut state = TaskReminderState::new();
        let reminder = state
            .build_reminder(TURNS_SINCE_WRITE_FIRST, &store)
            .await
            .expect("reminder exists");
        let text = reminder_text(&reminder);

        assert!(text.contains("Use TaskList only when the user asks to continue/resume"));
        assert!(!text.contains("Use TaskList to see details."));
    }
}
