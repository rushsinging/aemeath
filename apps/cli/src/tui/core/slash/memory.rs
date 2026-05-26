impl super::super::App {
    pub(super) fn show_memory_reminders(&mut self) {
        let lines = self
            .cmd_exec.session_reminders
            .lock()
            .ok()
            .map(|reminders| {
                reminders
                    .list()
                    .iter()
                    .map(|reminder| {
                        let marker = if reminder.done { "✓" } else { "□" };
                        format!("{marker} {} {}", reminder.id, reminder.content)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if lines.is_empty() {
            self.output_area.push_system("当前没有 session reminder。");
            return;
        }

        self.output_area.push_system("Session Reminders:");
        for line in lines {
            self.output_area.push_system(&line);
        }
    }
}
