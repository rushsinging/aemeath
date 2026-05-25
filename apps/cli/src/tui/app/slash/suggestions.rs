use crate::tui::completion::{generate_suggestions, SuggestionContext};
use ::runtime::api::command::CommandRegistry;

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

        let models: Vec<(String, String)> = self
            .cmd_exec.models_config
            .list_models()
            .into_iter()
            .map(|(p, m)| (p, if m.name.is_empty() { m.id } else { m.name }))
            .collect();

        let skills: Vec<(String, String, Vec<String>)> = self
            .skills
            .values()
            .map(|s| (s.name.clone(), s.description.clone(), s.aliases.clone()))
            .collect();

        // Build command list from CommandRegistry (single source of truth)
        let registry = CommandRegistry::global();
        let commands: Vec<(String, String, Vec<String>)> = registry
            .list()
            .into_iter()
            .map(|cmd| {
                (
                    cmd.name.clone(),
                    cmd.description.clone(),
                    cmd.aliases.clone(),
                )
            })
            .collect();

        let ctx = SuggestionContext {
            input,
            cursor_offset,
            cwd: self.session.cwd.clone(),
            models,
            skills,
            commands,
            sessions: self.session.cached_sessions.clone(),
        };

        let suggestions = generate_suggestions(&ctx);
        self.input_area.set_suggestions(suggestions);
    }
}
