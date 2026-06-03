#[cfg(test)]
use super::InputArea;

#[cfg(test)]
impl InputArea {
    /// Add a message to history in tests.
    pub(crate) fn add_history(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(pos) = self.history.iter().position(|s| s == text) {
            self.history.remove(pos);
        }
        self.history.push(text.to_string());
        const MAX_HISTORY: usize = 100;
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    /// Reset history navigation.
    pub(crate) fn reset_history_nav(&mut self) {
        self.history_index = None;
        self.saved_input.clear();
    }
}
