use crate::tui::model::input::completion::{generate_suggestions, SuggestionContext};
use crate::tui::model::input::completion_item::CompletionItem;

impl super::super::App {
    /// Update suggestions based on current input
    pub(crate) fn update_suggestions(&mut self) {
        let input = self.model.input.document.buffer.clone();
        let cursor_offset = self.model.input.document.cursor;

        // 读取启动期预取的模型缓存（refresh_model_cache），保持纯路径、避免每次按键 block_on。
        let models: Vec<(String, String)> = self
            .session
            .cached_models()
            .iter()
            .map(|m| {
                (
                    m.provider.clone(),
                    if m.name.is_empty() {
                        m.id.clone()
                    } else {
                        m.name.clone()
                    },
                )
            })
            .collect();

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
                    .map(|suggestion| {
                        CompletionItem::with_type(
                            &suggestion.display_text,
                            &suggestion.display_text,
                            suggestion.suggestion_type.clone(),
                        )
                    })
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
