use crate::tui::app::App;

impl App {
    pub(super) fn push_session_reminder_recap(&mut self) {
        let line = self
            .cmd_exec.session_reminders
            .lock()
            .ok()
            .and_then(|reminders| reminders.recap_line());

        if let Some(line) = line {
            self.output_area.push_system(&line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_session_reminder_recap_happy_path_appends_line() {
        let mut app = App::new(
            "session".to_string(),
            std::path::PathBuf::from("/tmp/aemeath-test"),
            "model".to_string(),
        );
        app.cmd_exec.session_reminders.lock().unwrap().add("任务一").unwrap();

        app.push_session_reminder_recap();

        assert!(app
            .output_area
            .lines
            .iter()
            .any(|line| line.content == "* recap: 任务一"));
    }

    #[test]
    fn test_push_session_reminder_recap_boundary_empty_does_not_append_line() {
        let mut app = App::new(
            "session".to_string(),
            std::path::PathBuf::from("/tmp/aemeath-test"),
            "model".to_string(),
        );
        let before = app.output_area.lines.len();

        app.push_session_reminder_recap();

        assert_eq!(app.output_area.lines.len(), before);
    }

    #[test]
    fn test_push_session_reminder_recap_completed_reminder_does_not_append_line() {
        let mut app = App::new(
            "session".to_string(),
            std::path::PathBuf::from("/tmp/aemeath-test"),
            "model".to_string(),
        );
        let id = app.session_reminders.lock().unwrap().add("任务一").unwrap();
        app.session_reminders.lock().unwrap().complete(&id).unwrap();
        let before = app.output_area.lines.len();

        app.push_session_reminder_recap();

        assert_eq!(app.output_area.lines.len(), before);
    }
}
