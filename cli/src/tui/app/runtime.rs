use super::{task_window, App};
use aemeath_core::message::Message;
use aemeath_core::skill::Skill;
use std::sync::Arc;

impl App {
    /// Reset per-conversation runtime state while preserving model/provider/session environment.
    pub(crate) async fn reset_runtime_state(&mut self) {
        self.total_input_tokens = 0;
        self.total_output_tokens = 0;
        self.total_api_calls = 0;
        self.last_input_tokens = 0;
        self.tool_call_active = false;
        self.is_processing = false;
        self.active_tool_call_ids.clear();
        self.input_queue.clear();
        self.output_area.reset_runtime_state();
        self.status_bar.reset_runtime_state();
        self.ask_user_reply_tx = None;
        self.ask_user_state = None;
        self.pending_reflection = None;
        self.turn_count = 0;
        if let Ok(mut reminders) = self.session_reminders.lock() {
            reminders.clear();
        }
        // Clear task store so stale tasks don't leak into new conversations
        if let Some(ref ts) = self.task_store {
            ts.clear().await;
        }
    }

    /// Set loaded skills for slash command alias lookup
    pub fn set_skills(&mut self, skills: std::collections::HashMap<String, Skill>) {
        self.skills = skills;
    }

    /// Find a skill by its name or alias
    pub(crate) fn find_skill_by_alias(&self, alias: &str) -> Option<&Skill> {
        self.skills
            .values()
            .find(|s| s.name == alias || s.aliases.iter().any(|a| a == alias))
    }

    /// Update task status display in output area. Also runs lifecycle checks.
    pub(crate) async fn update_task_status(
        &mut self,
        task_store: &Arc<aemeath_core::task::TaskStore>,
        _is_processing: bool,
    ) {
        let tasks = task_store.list_current_batch().await;
        let active: Vec<_> = tasks
            .iter()
            .filter(|t| t.status != aemeath_core::task::TaskStatus::Deleted)
            .cloned()
            .collect();

        if active.is_empty() {
            // Check lifecycle: if previous batch was completed and auto-cleared
            self.output_area.set_task_status(Vec::new());
        } else {
            let display_map = task_store.get_batch_display_map().await;
            let task_list_config = aemeath_core::config::TaskListConfig::default();
            let lines =
                task_window::build_task_window(&active, &display_map, task_list_config.max_lines);
            self.output_area.set_task_status(lines);
        }
    }

    /// Build a Session from current state, including task snapshot.
    pub(crate) async fn build_session(
        &self,
        messages: Vec<Message>,
    ) -> aemeath_core::session::Session {
        use aemeath_core::session::{now_iso, Session};
        let task_snapshot = match &self.task_store {
            Some(ts) => {
                let snap = ts.snapshot().await;
                if snap.tasks.is_empty() {
                    None
                } else {
                    Some(snap)
                }
            }
            None => None,
        };
        Session {
            id: self.session_id.clone(),
            cwd: self.cwd.to_string_lossy().to_string(),
            messages,
            created_at: self.session_created_at.clone().unwrap_or_else(now_iso),
            updated_at: now_iso(),
            metadata: Default::default(),
            tasks: task_snapshot,
            workspace: self.workspace_context.clone(),
        }
    }

    /// Refresh the cached session list for /resume autocomplete
    pub async fn refresh_session_cache(&mut self) {
        let sessions = aemeath_core::session::list_sessions().await;
        self.cached_sessions = sessions
            .iter()
            .take(20)
            .map(|s| {
                let summary = build_session_summary(s);
                (s.id.clone(), summary)
            })
            .collect();
    }
}

/// Build a one-line summary for a session, shown in /resume autocomplete
fn build_session_summary(session: &aemeath_core::session::Session) -> String {
    format!("{} [{}msg]", session.summary(), session.messages.len())
}
