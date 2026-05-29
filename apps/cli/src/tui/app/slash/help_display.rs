use super::help::SLASH_HELP_LINES;

impl super::super::App {
    pub(super) fn show_slash_help(&mut self) {
        self.append_system_notice(SLASH_HELP_LINES.join("\n"));
    }
}
