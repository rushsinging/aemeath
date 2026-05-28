use crate::tui::completion::{generate_suggestions, SuggestionContext};

impl super::super::App {
    /// Update suggestions based on current input
    pub(crate) fn update_suggestions(&mut self) {
        let input = self.input_area.get_text();
        let (_row, col) = self.input_area.cursor_position();
        // Convert column (char count) to byte offset
        let cursor_offset = input
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(input.len());

        let models: Vec<(String, String)> = if let Some(agent_client) = &self.agent_client {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(agent_client.list_models())
                    .unwrap_or_default()
            })
            .into_iter()
            .map(|m| (m.provider, if m.name.is_empty() { m.id } else { m.name }))
            .collect()
        } else {
            Vec::new()
        };

        let skills: Vec<(String, String, Vec<String>)> = self
            .skills
            .values()
            .map(|s| {
                (
                    s.name.clone(),
                    s.description.clone().unwrap_or_default(),
                    s.aliases.clone(),
                )
            })
            .collect();

        let commands = sdk::builtin_commands();

        let ctx = SuggestionContext {
            input,
            cursor_offset,
            cwd: self.session.cwd.clone(),
            models,
            skills,
            commands,
            sessions: self.session.cached_sessions().to_vec(),
        };

        let suggestions = generate_suggestions(&ctx);
        self.input_area.set_suggestions(suggestions);
    }
}
