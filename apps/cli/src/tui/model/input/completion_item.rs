use super::completion::SuggestionType;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    pub replacement: String,
    pub suggestion_type: SuggestionType,
}

impl CompletionItem {
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
mod tests {
    use super::*;

    #[test]
    fn test_completion_item_stores_label() {
        let item = CompletionItem::new("/help", "/help");
        assert_eq!(item.label, "/help");
    }

    #[test]
    fn test_completion_item_stores_replacement() {
        let item = CompletionItem::new("Help", "/help");
        assert_eq!(item.replacement, "/help");
    }

    #[test]
    fn test_completion_item_allows_empty_replacement() {
        let item = CompletionItem::new("empty", "");
        assert_eq!(item.replacement, "");
    }
}
