// Suggestion 在 apply_current_suggestion 中通过 self.input_area.accept_suggestion() 使用

impl super::App {
    /// Copy text to clipboard
    #[allow(dead_code)]
    pub fn copy_to_clipboard(&mut self, text: &str) -> Result<(), String> {
        let mut cmd = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn pbcopy: {e}"))?;
        use std::io::Write;
        cmd.stdin.take()
            .ok_or_else(|| "Failed to open stdin".to_string())?
            .write_all(text.as_bytes())
            .map_err(|e| format!("Failed to write to clipboard: {e}"))?;
        cmd.wait().map_err(|e| format!("Failed to wait for pbcopy: {e}"))?;
        Ok(())
    }

    /// Accept the currently highlighted suggestion
    pub fn apply_current_suggestion(&mut self) {
        if let Some(suggestion) = self.input_area.accept_suggestion() {
            let text = &suggestion.display_text;
            self.input_area.clear();
            for ch in text.chars() {
                self.input_area.input(ch);
            }
        }
    }
}
