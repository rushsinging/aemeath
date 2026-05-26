use ratatui::{style::Style, text::Line};

use crate::tui::display::theme;

impl super::OutputArea {
    /// 渲染排队消息，保留消息内换行并为后续行补齐缩进
    pub(super) fn build_queued_message_lines(&self) -> Vec<Line<'static>> {
        let style = Style::default().fg(theme::TEXT_DIM);
        let mut lines = Vec::new();

        for msg in &self.queued_messages {
            let mut parts = msg.split('\n');
            let first = parts.next().unwrap_or("");
            lines.push(Line::styled(format!("> {first}"), style));

            for part in parts {
                lines.push(Line::styled(format!("  {part}"), style));
            }
        }

        lines
    }
}
