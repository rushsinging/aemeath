use sdk::{AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView};

use crate::tui::output_area::{LineStyle, OutputLine, INDENT};

use super::common::format_agent_tool_calls;

impl super::super::OutputArea {
    pub fn push_agent_progress(&mut self, tool_id: &str, event: AgentProgressEventView) {
        match event.kind {
            AgentProgressKindView::ToolCalls { calls } => {
                self.push_agent_tool_calls(tool_id, &calls)
            }
            AgentProgressKindView::Message { text } => {
                self.push_tool_progress(tool_id, &text);
            }
        }
    }

    fn push_agent_tool_calls(&mut self, tool_id: &str, calls: &[AgentToolCallProgressView]) {
        self.finish_streaming();
        let summary = format_agent_tool_calls(calls);
        let content = format!("{INDENT}↳ {summary}");
        if let Some(line) = self.lines.iter_mut().rev().find(|line| {
            line.tool_id.as_deref() == Some(tool_id)
                && line.content.starts_with(&format!("{INDENT}↳ "))
        }) {
            line.content = content;
            line.style = LineStyle::System;
            return;
        }

        let progress_line = OutputLine {
            content,
            style: LineStyle::System,
            tool_id: Some(tool_id.to_string()),
            spans: None,
        };
        let insert_at = self.tool_insert_position(tool_id);
        self.insert_lines_at(insert_at, vec![progress_line]);
    }

    pub fn push_tool_progress(&mut self, tool_id: &str, text: &str) {
        self.finish_streaming();

        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        let content = format!("{INDENT}↳ {trimmed}");
        let already_shown = self
            .lines
            .iter()
            .rev()
            .take(8)
            .any(|line| line.tool_id.as_deref() == Some(tool_id) && line.content == content);
        if already_shown {
            return;
        }

        let progress_line = OutputLine {
            content,
            style: LineStyle::System,
            tool_id: Some(tool_id.to_string()),
            spans: None,
        };

        let insert_at = self.tool_insert_position(tool_id);
        self.insert_lines_at(insert_at, vec![progress_line]);
    }

    pub(super) fn tool_insert_position(&self, tool_id: &str) -> usize {
        self.lines
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| line.tool_id.as_deref() == Some(tool_id))
            .map(|(idx, _)| idx + 1)
            .unwrap_or(self.lines.len())
    }
}
