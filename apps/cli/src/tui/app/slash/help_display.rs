use super::help::SLASH_HELP_LINES;

impl super::super::App {
    pub(super) fn show_slash_help(&mut self) {
        for line in SLASH_HELP_LINES {
            self.output_area.push_system(line);
        }
    }
}
