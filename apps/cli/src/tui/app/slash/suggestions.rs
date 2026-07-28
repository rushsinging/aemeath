use crate::tui::model::input::completion::{generate_suggestions, SuggestionContext};
use crate::tui::model::input::completion_item::CompletionItem;
use crate::tui::update::intent::AgentIntent;

impl super::super::App {
    /// Update suggestions based on current input
    pub(crate) fn update_suggestions(&mut self) {
        let input = self.model.input.document.buffer.clone();
        let cursor_offset = self.model.input.document.cursor;

        // #567：模型列表走事件流（ListModels），缓存尚未接入。暂传空列表。
        let models: Vec<(String, String)> = Vec::new();

        let mut commands = self
            .command_catalog
            .as_deref()
            .map(|catalog| {
                catalog
                    .list()
                    .into_iter()
                    .map(|command| {
                        (
                            command.name.as_str().to_string(),
                            command.description,
                            command
                                .aliases
                                .into_iter()
                                .map(|alias| alias.as_str().to_string())
                                .collect(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        commands.extend(
            self.skill_completion_catalog
                .entries
                .iter()
                .filter_map(|skill| {
                    Some((
                        skill.slash_command.clone()?,
                        match &skill.argument_hint {
                            Some(hint) => format!("{} {hint}", skill.description),
                            None => skill.description.clone(),
                        },
                        skill.slash_aliases.clone(),
                    ))
                }),
        );

        let ctx = SuggestionContext {
            input,
            cursor_offset,
            cwd: self.session.cwd.clone(),
            models,
            commands,
            sessions: Vec::new(),
        };

        let suggestions = generate_suggestions(&ctx);
        // Completion changes update the model only; InputArea renders from model-derived state.
        self.apply_agent_intent(AgentIntent::Input(
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
        ));
    }
}
