use super::completion::SuggestionType;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    pub replacement: String,
    pub suggestion_type: SuggestionType,
}

impl CompletionItem {
    #[cfg(test)]
    pub fn new(label: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self::with_type(label, replacement, SuggestionType::Command)
    }

    pub fn with_type(
        label: impl Into<String>,
        replacement: impl Into<String>,
        suggestion_type: SuggestionType,
    ) -> Self {
        Self {
            label: label.into(),
            replacement: replacement.into(),
            suggestion_type,
        }
    }
}

#[cfg(test)]
#[path = "completion_item_tests.rs"]
mod tests;
