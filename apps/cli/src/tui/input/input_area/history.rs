use super::InputArea;

impl InputArea {
    /// Add a message to history in tests.
    #[cfg(test)]
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

    /// Navigate up in history (older messages)
    #[cfg(test)]
    pub(crate) fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }

        if self.history_index.is_none() {
            self.saved_input = self.get_text();
            self.history_index = Some(self.history.len());
        }

        if let Some(idx) = self.history_index {
            if idx > 0 {
                let new_idx = idx - 1;
                self.history_index = Some(new_idx);
                let text = self.history[new_idx].clone();
                self.set_text(&text);
            }
        }
    }

    /// Navigate down in history (newer messages)
    #[cfg(test)]
    pub(crate) fn history_down(&mut self) {
        if self.history.is_empty() || self.history_index.is_none() {
            return;
        }

        if let Some(idx) = self.history_index {
            if idx < self.history.len() - 1 {
                let new_idx = idx + 1;
                self.history_index = Some(new_idx);
                let text = self.history[new_idx].clone();
                self.set_text(&text);
            } else {
                let text = self.saved_input.clone();
                self.set_text(&text);
                self.history_index = None;
            }
        }
    }

    /// Reset history navigation
    #[cfg(test)]
    pub(crate) fn reset_history_nav(&mut self) {
        self.history_index = None;
        self.saved_input.clear();
    }
}
