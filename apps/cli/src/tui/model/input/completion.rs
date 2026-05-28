use super::completion_item::CompletionItem;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputCompletion {
    pub visible: bool,
    pub selected_index: Option<usize>,
    pub query: String,
    pub items: Vec<CompletionItem>,
}

impl InputCompletion {
    pub fn set_items(&mut self, items: Vec<CompletionItem>, query: String) {
        self.visible = !items.is_empty();
        self.selected_index = self.visible.then_some(0);
        self.items = items;
        self.query = query;
    }

    pub fn clear(&mut self) {
        self.visible = false;
        self.selected_index = None;
        self.items.clear();
        self.query.clear();
    }

    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            self.selected_index = None;
            return;
        }
        let current = self.selected_index.unwrap_or(0);
        self.selected_index = Some((current + 1) % self.items.len());
    }

    pub fn select_previous(&mut self) {
        if self.items.is_empty() {
            self.selected_index = None;
            return;
        }
        let current = self.selected_index.unwrap_or(0);
        self.selected_index = Some(if current == 0 {
            self.items.len() - 1
        } else {
            current - 1
        });
    }

    pub fn selected_item(&self) -> Option<&CompletionItem> {
        self.selected_index.and_then(|index| self.items.get(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_set_items_selects_first() {
        let mut completion = InputCompletion::default();
        completion.set_items(vec![CompletionItem::new("a", "a")], "a".to_string());
        assert_eq!(completion.selected_index, Some(0));
    }

    #[test]
    fn test_completion_set_empty_hides() {
        let mut completion = InputCompletion::default();
        completion.set_items(Vec::new(), "".to_string());
        assert!(!completion.visible);
    }

    #[test]
    fn test_completion_select_next_wraps() {
        let mut completion = InputCompletion::default();
        completion.set_items(
            vec![CompletionItem::new("a", "a"), CompletionItem::new("b", "b")],
            "".to_string(),
        );
        completion.select_next();
        completion.select_next();
        assert_eq!(completion.selected_index, Some(0));
    }
}
