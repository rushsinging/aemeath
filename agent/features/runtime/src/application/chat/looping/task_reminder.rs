//! Task reminder passive injection.
//!
//! Automatically injects a concise task status reminder as a `<system-reminder>`
//! user message every N turns. This lets the LLM perceive progress without
//! actively calling `TaskList`.

use share::message::{ContentBlock, Message, Role};
use task::{TaskAccess, TaskStatus};

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
    /// 3. The current batch still has pending/in-progress tasks
    ///
    /// Returns `None` if throttled or the current batch has no open tasks.
    ///
    /// #889：改用 low-privilege `TaskAccess`（`reminder_snapshot` + `list_batches`），
    /// 只汇报 current batch 的开放任务。
    ///
    /// `lang` selects the template language (`"en"` / `"zh"`, defaults to `"en"`).
    pub async fn build_reminder(
        &mut self,
        current_turn: u64,
        access: &dyn TaskAccess,
        lang: &str,
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

        let no_summary: &str = match lang {
            "zh" => "无摘要",
            _ => "no summary",
        };

        // Current batch 的开放任务（Pending / InProgress）。
        let snapshot = access.reminder_snapshot();
        let current_batch = snapshot.current_batch?;
        let open_items: Vec<_> = snapshot
            .items
            .iter()
            .filter(|item| matches!(item.status, TaskStatus::Pending | TaskStatus::InProgress))
            .collect();
        if open_items.is_empty() {
            return None;
        }
        let batches = access.list_batches();
        let batch = batches.iter().find(|batch| batch.id() == current_batch)?;
        let task_text = open_items
            .iter()
            .map(|item| format!("#{} {} [{:?}]", item.id, item.subject, item.status))
            .collect::<Vec<_>>()
            .join(", ");
        let summary = batch.summary().unwrap_or(no_summary);
        let batch_line = format!(
            "Task batch #{} [{:?}] — summary: {} — {}",
            current_batch,
            batch.status(),
            summary,
            task_text
        );

        self.last_reminder_turn = current_turn;

        let (preamble, epilogue) = match lang {
            "zh" => (
                "任务提醒按 task batch 分组，可能属于较早的用户请求。如果与最新的用户消息无关，请优先回答最新的用户消息。\n",
                "\n仅当用户要求继续/恢复，或列出的任务明显相关时，才使用 TaskList。",
            ),
            _ => (
                "Task reminders are grouped by task batch and may belong to earlier user requests. If unrelated to the latest user message, answer the latest user message first.\n",
                "\nUse TaskList only when the user asks to continue/resume or when a listed task is clearly relevant.",
            ),
        };
        let text = format!("{}{}\n{}", preamble, batch_line, epilogue);
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
    use task::{BatchCreateSpec, TaskCreateSpec, TaskId, TaskPriority, TaskStore};

    fn reminder_text(message: &Message) -> String {
        message.text_content()
    }

    /// Build a store with an active batch (`summary`) holding a single task.
    /// Returns the store and the created task id.
    fn store_with_task(summary: &str, subject: &str) -> (TaskStore, TaskId) {
        let store = TaskStore::new();
        let id = {
            let access: &dyn TaskAccess = &store;
            access
                .create_batch(BatchCreateSpec::try_new(summary.to_owned()).unwrap(), 1)
                .unwrap();
            access
                .create_task(
                    TaskCreateSpec::try_new(
                        subject.to_owned(),
                        String::new(),
                        None,
                        TaskPriority::Normal,
                    )
                    .unwrap(),
                    2,
                )
                .unwrap()
                .value
                .id()
        };
        (store, id)
    }

    #[tokio::test]
    async fn test_build_reminder_first_throttled_before_threshold() {
        let (store, _) = store_with_task("old request", "遗留任务");

        let mut state = TaskReminderState::new();
        // Turn 2 < TURNS_SINCE_WRITE_FIRST(3) → should NOT trigger
        let reminder = state.build_reminder(2, &store, "en").await;
        assert!(reminder.is_none());
    }

    #[tokio::test]
    async fn test_build_reminder_first_triggers_at_threshold() {
        let (store, _) = store_with_task("old request", "遗留任务");

        let mut state = TaskReminderState::new();
        // Turn 3 == TURNS_SINCE_WRITE_FIRST(3) → should trigger
        let reminder = state
            .build_reminder(TURNS_SINCE_WRITE_FIRST, &store, "en")
            .await
            .expect("reminder exists");
        let text = reminder_text(&reminder);
        assert!(text.contains("Task batch"));
    }

    #[tokio::test]
    async fn test_build_reminder_groups_by_batch_and_warns_unrelated() {
        let (store, task_id) = store_with_task("修复 task 状态", "分析旧任务");
        let mut state = TaskReminderState::new();
        let reminder = state
            .build_reminder(TURNS_SINCE_WRITE_FIRST, &store, "en")
            .await
            .expect("reminder exists");
        let text = reminder_text(&reminder);

        assert!(text.contains("Task batch #"));
        assert!(text.contains("修复 task 状态"));
        assert!(text.contains("may belong to earlier user requests"));
        assert!(text.contains("If unrelated to the latest user message"));
        let access: &dyn TaskAccess = &store;
        assert_eq!(access.get(task_id).unwrap().status(), TaskStatus::Pending);
    }

    #[tokio::test]
    async fn test_build_reminder_omits_all_completed_batches() {
        let (store, task_id) = store_with_task("done", "完成任务");
        {
            let access: &dyn TaskAccess = &store;
            access
                .transition(task_id, TaskStatus::Completed, 3)
                .unwrap();
        }

        let mut state = TaskReminderState::new();
        let reminder = state
            .build_reminder(TURNS_SINCE_WRITE_FIRST, &store, "en")
            .await;

        assert!(reminder.is_none());
    }

    #[tokio::test]
    async fn test_build_reminder_mentions_use_tasklist_only_when_relevant() {
        let (store, _) = store_with_task("old request", "遗留任务");

        let mut state = TaskReminderState::new();
        let reminder = state
            .build_reminder(TURNS_SINCE_WRITE_FIRST, &store, "en")
            .await
            .expect("reminder exists");
        let text = reminder_text(&reminder);

        assert!(text.contains("Use TaskList only when the user asks to continue/resume"));
        assert!(!text.contains("Use TaskList to see details."));
    }

    #[tokio::test]
    async fn test_build_reminder_zh_template() {
        let (store, _) = store_with_task("old request", "遗留任务");

        let mut state = TaskReminderState::new();
        let reminder = state
            .build_reminder(TURNS_SINCE_WRITE_FIRST, &store, "zh")
            .await
            .expect("reminder exists");
        let text = reminder_text(&reminder);

        assert!(text.contains("任务提醒按 task batch 分组"));
        assert!(text.contains("仅当用户要求继续/恢复"));
        assert!(!text.contains("Use TaskList only when"));
    }
}
