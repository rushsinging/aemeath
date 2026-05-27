use super::App;

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
        if let Some(agent_client) = &self.agent_client {
            if let Err(e) = agent_client.sync_current_messages(Vec::new()).await {
                log::warn!("failed to reset SDK session messages: {e}");
            }
        }
    }

    /// Set loaded skills for slash command alias lookup
    pub fn set_skills(&mut self, skills: std::collections::HashMap<String, sdk::SkillView>) {
        self.skills = skills;
    }

    /// Find a skill by its name or alias
    pub(crate) fn find_skill_by_alias(&self, alias: &str) -> Option<&sdk::SkillView> {
        self.skills
            .values()
            .find(|s| s.name == alias || s.aliases.iter().any(|a| a == alias))
    }

    /// Update task status display in output area. Also runs lifecycle checks.
    pub(crate) async fn update_task_status(&mut self, _is_processing: bool) {
        let Some(agent_client) = &self.agent_client else {
            self.output_area.set_task_status(Vec::new());
            return;
        };
        match agent_client.task_status().await {
            Ok(view) => self.output_area.set_task_status(view.lines),
            Err(e) => log::warn!("failed to fetch SDK task status: {e}"),
        }
    }

    /// Refresh the cached session list for /resume autocomplete
    pub async fn refresh_session_cache(&mut self) {
        if let Some(agent_client) = &self.agent_client {
            if let Ok(sessions) = agent_client.list_sessions().await {
                self.session.cached_sessions = sessions
                    .iter()
                    .take(20)
                    .map(|s| {
                        let summary = format!("{} [{}msg]", s.summary, s.message_count);
                        (s.id.clone(), summary)
                    })
                    .collect();
            }
        }
    }
}
