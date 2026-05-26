use super::App;
use crate::tui::display::task_window;
use ::runtime::api::core::skill_ops::Skill;
use std::sync::Arc;

impl App {
    /// Reset per-conversation runtime state while preserving model/provider/session environment.
    pub(crate) async fn reset_runtime_state(&mut self) {
        self.chat.total_input_tokens = 0;
        self.chat.total_output_tokens = 0;
        self.chat.total_api_calls = 0;
        self.chat.last_input_tokens = 0;
        self.chat.tool_call_active = false;
        self.chat.is_processing = false;
        self.chat.active_tool_call_ids.clear();
        self.input.input_queue.clear();
        self.output_area.reset_runtime_state();
        self.status_bar.reset_runtime_state();
        self.input.ask_user_reply_tx = None;
        self.input.ask_user_state = None;
        self.chat.pending_reflection = None;
        self.chat.turn_count = 0;
        if let Ok(mut reminders) = self.cmd_exec.session_reminders.lock() {
            reminders.clear();
        }
        // Clear task store so stale tasks don't leak into new conversations
        if let Some(ref ts) = self.cmd_exec.task_store {
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
        task_store: &Arc<::runtime::api::core::task::TaskStore>,
        _is_processing: bool,
    ) {
        if let Some(agent_client) = &self.agent_client {
            match agent_client.task_status().await {
                Ok(view) => {
                    self.output_area.set_task_status(view.lines);
                    return;
                }
                Err(e) => log::warn!("failed to fetch SDK task status: {e}"),
            }
        }
        let tasks = task_store.list_current_batch().await;
        let active: Vec<_> = tasks
            .iter()
            .filter(|t| t.status != ::runtime::api::core::task::TaskStatus::Deleted)
            .cloned()
            .collect();

        if active.is_empty() {
            // Check lifecycle: if previous batch was completed and auto-cleared
            self.output_area.set_task_status(Vec::new());
        } else {
            let display_map = task_store.get_batch_display_map().await;
            let task_list_config = ::runtime::api::core::config::TaskListConfig::default();
            let lines =
                task_window::build_task_window(&active, &display_map, task_list_config.max_lines);
            self.output_area.set_task_status(lines);
        }
    }

    /// Refresh the cached session list for /resume autocomplete
    pub async fn refresh_session_cache(&mut self) {
        let sessions = ::runtime::api::session::list_sessions().await;
        self.session.cached_sessions = sessions
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
fn build_session_summary(session: &::runtime::api::session::Session) -> String {
    format!("{} [{}msg]", session.summary(), session.messages.len())
}
