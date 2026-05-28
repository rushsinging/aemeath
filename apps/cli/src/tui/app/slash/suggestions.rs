use crate::tui::adapter::input_widget::completion_item_from_suggestion;
use crate::tui::model::input::completion::{generate_suggestions, SuggestionContext};

impl super::super::App {
    /// Update suggestions based on current input
    pub(crate) fn update_suggestions(&mut self) {
        let input = self.model.input.document.buffer.clone();
        let cursor_offset = self.model.input.document.cursor;

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
        let changes = self.model.input.apply(
            crate::tui::model::input::intent::InputIntent::SetCompletions {
                query: ctx.input.clone(),
                items: suggestions
                    .iter()
                    .map(completion_item_from_suggestion)
                    .collect(),
            },
        );
        crate::tui::adapter::input_widget::apply_input_changes_to_widget(
            &mut self.input_area,
            &mut self.status_bar,
            &changes,
        );
    }
}
