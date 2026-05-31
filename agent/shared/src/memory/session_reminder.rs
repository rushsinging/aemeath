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

    pub fn add(
        &mut self,
        id: impl Into<String>,
        content: impl Into<String>,
        created_at: u64,
    ) -> MemoryResult<String> {
        let id = id.into();
        let content = content.into();
        if id.trim().is_empty() {
            return Err(MemoryError::invalid_input("reminder id 不能为空"));
        }
        if content.trim().is_empty() {
            return Err(MemoryError::invalid_input("reminder 内容不能为空"));
        }

        self.reminders.push(SessionReminder {
            id: id.clone(),
            content,
            done: false,
            created_at,
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
        let id = reminders.add("reminder-1", "处理 /clear bug", 123).unwrap();

        assert_eq!(id, "reminder-1");
        assert_eq!(reminders.list().len(), 1);
        assert_eq!(reminders.list()[0].content, "处理 /clear bug");
        assert_eq!(reminders.list()[0].created_at, 123);
        assert!(!reminders.list()[0].done);
    }

    #[test]
    fn test_session_reminders_add_empty_error() {
        let mut reminders = SessionReminders::new();
        let result = reminders.add("reminder-1", "   ", 123);

        assert!(matches!(result, Err(MemoryError::InvalidInput { .. })));
        assert!(reminders.list().is_empty());
    }

    #[test]
    fn test_session_reminders_add_empty_id_error() {
        let mut reminders = SessionReminders::new();
        let result = reminders.add("   ", "处理 /clear bug", 123);

        assert!(matches!(result, Err(MemoryError::InvalidInput { .. })));
        assert!(reminders.list().is_empty());
    }

    #[test]
    fn test_session_reminders_complete() {
        let mut reminders = SessionReminders::new();
        let id = reminders.add("reminder-1", "测试 reminder", 123).unwrap();

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
        reminders.add("reminder-1", "任务一", 123).unwrap();
        reminders.add("reminder-2", "任务二", 124).unwrap();

        assert_eq!(
            reminders.recap_line().as_deref(),
            Some("* recap: 任务一 | 任务二")
        );
    }

    #[test]
    fn test_session_reminders_clear() {
        let mut reminders = SessionReminders::new();
        reminders.add("reminder-1", "任务一", 123).unwrap();

        reminders.clear();

        assert!(reminders.list().is_empty());
        assert!(reminders.recap_line().is_none());
    }
}
