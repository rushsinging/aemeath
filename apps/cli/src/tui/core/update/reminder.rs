use crate::tui::core::App;

impl App {
    pub(super) fn handle_reminder_recap(&mut self, line: &str) {
        self.output_area.push_system(line);
    }

    pub(super) fn handle_memory_list(&mut self, reminders: &[sdk::ReminderView]) {
        if reminders.is_empty() {
            self.output_area.push_system("当前没有 session reminder。");
        } else {
            self.output_area.push_system("Session Reminders:");
            for r in reminders {
                let marker = if r.done { "✓" } else { "□" };
                self.output_area
                    .push_system(&format!("{marker} {} {}", r.id, r.content));
            }
        }
    }
}
