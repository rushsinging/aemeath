#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TaskStatusSnapshot {
    pub session_id: Option<String>,
    pub revision: u64,
    pub state: Option<crate::tui::adapter::runtime_view::TuiTaskState>,
    pub total: usize,
    pub completed: usize,
    pub in_progress: usize,
    pub lines: Vec<String>,
}

impl TaskStatusSnapshot {
    pub fn replace(&mut self, state: crate::tui::adapter::runtime_view::TuiTaskState) -> bool {
        if self.session_id.as_deref() == Some(state.session_id.as_str())
            && state.revision < self.revision
        {
            return false;
        }
        let lines = render_task_state(&state);
        self.session_id = Some(state.session_id.clone());
        self.revision = state.revision;
        self.total = state.total;
        self.completed = state.completed;
        self.in_progress = state.in_progress;
        self.state = Some(state);
        self.lines = lines;
        true
    }
}

fn render_task_state(state: &crate::tui::adapter::runtime_view::TuiTaskState) -> Vec<String> {
    if state.current_batch.is_none() {
        return Vec::new();
    }
    let mut lines = vec![format!("━━ Tasks: {}/{} ━━", state.completed, state.total)];
    lines.extend(state.items.iter().map(|item| {
        let icon = match item.status {
            crate::tui::adapter::runtime_view::TuiTaskItemStatus::Completed => "✓",
            crate::tui::adapter::runtime_view::TuiTaskItemStatus::InProgress => "■",
            crate::tui::adapter::runtime_view::TuiTaskItemStatus::Pending => "□",
        };
        let blocked_by = if item.blocked_by_sequences.is_empty() {
            String::new()
        } else {
            format!(
                " (blocked by {})",
                item.blocked_by_sequences
                    .iter()
                    .map(|sequence| format!("#{sequence}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        format!("{} #{} {}{}", icon, item.sequence, item.subject, blocked_by)
    }));
    if state.hidden_count > 0 {
        lines.push(format!("… +{} more", state.hidden_count));
    }
    lines
}

#[cfg(test)]
#[path = "task_status_tests.rs"]
mod tests;
