use super::help::command_help_lines;

impl super::super::App {
    pub(super) fn show_slash_help(&mut self) {
        let Some(catalog) = self.command_catalog.as_deref() else {
            self.append_error_notice("Command catalog unavailable.");
            return;
        };
        self.append_system_notice(command_help_lines(catalog).join("\n"));
    }
}
