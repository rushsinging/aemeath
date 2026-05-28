#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionItem {
    pub label: String,
    pub replacement: String,
}

impl CompletionItem {
    pub fn new(label: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            replacement: replacement.into(),
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
