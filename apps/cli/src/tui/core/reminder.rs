/// TUI 本地 session reminder 类型（独立于 runtime，仅用于同步展示）。
#[derive(Debug, Default, Clone)]
pub(crate) struct SessionReminder {
    pub id: String,
    pub content: String,
    pub done: bool,
    pub created_at: u64,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct SessionReminders {
    reminders: Vec<SessionReminder>,
}

impl SessionReminders {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, content: impl Into<String>) -> Result<String, String> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err("reminder 内容不能为空".to_string());
        }
        let id = uuid::Uuid::now_v7().to_string();
        self.reminders.push(SessionReminder {
            id: id.clone(),
            content,
            done: false,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
        Ok(id)
    }

    pub fn complete(&mut self, id: &str) -> Result<(), String> {
        let reminder = self
            .reminders
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| format!("memory not found: {id}"))?;
        reminder.done = true;
        Ok(())
    }

    pub fn list(&self) -> &[SessionReminder] {
        &self.reminders
    }

    pub fn recap_line(&self) -> Option<String> {
        let active: Vec<&str> = self
            .reminders
            .iter()
            .filter(|r| !r.done)
            .map(|r| r.content.as_str())
            .collect();
        if active.is_empty() {
            None
        } else {
            Some(format!("* recap: {}", active.join(" | ")))
        }
    }
}
