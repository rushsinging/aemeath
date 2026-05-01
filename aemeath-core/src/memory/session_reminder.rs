use super::entry::current_timestamp_secs;
use super::error::{MemoryError, MemoryResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionReminder {
    pub id: String,
    pub content: String,
    pub done: bool,
    pub created_at: u64,
}

#[derive(Debug, Default, Clone)]
pub struct SessionReminders {
    reminders: Vec<SessionReminder>,
}

impl SessionReminders {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, content: impl Into<String>) -> MemoryResult<String> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(MemoryError::invalid_input("reminder 内容不能为空"));
        }

        let id = uuid::Uuid::now_v7().to_string();
        self.reminders.push(SessionReminder {
            id: id.clone(),
            content,
            done: false,
            created_at: current_timestamp_secs(),
        });
        Ok(id)
    }

    pub fn complete(&mut self, id: &str) -> MemoryResult<()> {
        let reminder = self
            .reminders
            .iter_mut()
            .find(|reminder| reminder.id == id)
            .ok_or_else(|| MemoryError::not_found(id))?;
        reminder.done = true;
        Ok(())
    }

    pub fn list(&self) -> &[SessionReminder] {
        &self.reminders
    }

    pub fn clear(&mut self) {
        self.reminders.clear();
    }

    pub fn recap_line(&self) -> Option<String> {
        let active = self
            .reminders
            .iter()
            .filter(|reminder| !reminder.done)
            .map(|reminder| reminder.content.as_str())
            .collect::<Vec<_>>();

        if active.is_empty() {
            None
        } else {
            Some(format!("* recap: {}", active.join(" | ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_reminders_add() {
        let mut reminders = SessionReminders::new();
        let id = reminders.add("处理 /clear bug").unwrap();

        assert!(!id.is_empty());
        assert_eq!(reminders.list().len(), 1);
        assert_eq!(reminders.list()[0].content, "处理 /clear bug");
        assert!(!reminders.list()[0].done);
    }

    #[test]
    fn test_session_reminders_add_empty_error() {
        let mut reminders = SessionReminders::new();
        let result = reminders.add("   ");

        assert!(matches!(result, Err(MemoryError::InvalidInput { .. })));
        assert!(reminders.list().is_empty());
    }

    #[test]
    fn test_session_reminders_complete() {
        let mut reminders = SessionReminders::new();
        let id = reminders.add("测试 reminder").unwrap();

        reminders.complete(&id).unwrap();

        assert!(reminders.list()[0].done);
        assert!(reminders.recap_line().is_none());
    }

    #[test]
    fn test_session_reminders_complete_not_found() {
        let mut reminders = SessionReminders::new();
        let result = reminders.complete("missing");

        assert!(matches!(result, Err(MemoryError::NotFound { .. })));
    }

    #[test]
    fn test_session_reminders_recap_line() {
        let mut reminders = SessionReminders::new();
        reminders.add("任务一").unwrap();
        reminders.add("任务二").unwrap();

        assert_eq!(reminders.recap_line().as_deref(), Some("* recap: 任务一 | 任务二"));
    }

    #[test]
    fn test_session_reminders_clear() {
        let mut reminders = SessionReminders::new();
        reminders.add("任务一").unwrap();

        reminders.clear();

        assert!(reminders.list().is_empty());
        assert!(reminders.recap_line().is_none());
    }
}
