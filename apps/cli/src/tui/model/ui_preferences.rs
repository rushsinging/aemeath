use crate::tui::view_model::markdown_spacing::MarkdownSpacingPolicy;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiPreferencesIntent {
    MarkdownSpacingChanged(MarkdownSpacingPolicy),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UiPreferencesChange {
    #[default]
    Unchanged,
    MarkdownSpacingChanged,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UiPreferences {
    markdown_spacing: MarkdownSpacingPolicy,
}

impl UiPreferences {
    pub const fn markdown_spacing(&self) -> MarkdownSpacingPolicy {
        self.markdown_spacing
    }

    pub fn apply(&mut self, intent: UiPreferencesIntent) -> UiPreferencesChange {
        match intent {
            UiPreferencesIntent::MarkdownSpacingChanged(policy)
                if self.markdown_spacing != policy =>
            {
                self.markdown_spacing = policy;
                UiPreferencesChange::MarkdownSpacingChanged
            }
            UiPreferencesIntent::MarkdownSpacingChanged(_) => UiPreferencesChange::Unchanged,
        }
    }
}

#[cfg(test)]
#[path = "ui_preferences_tests.rs"]
mod tests;
