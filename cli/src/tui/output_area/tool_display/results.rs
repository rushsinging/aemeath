use crate::tui::output_area::{build_diff_lines, LineStyle, OutputLine, INDENT};

use super::lookup_display;

impl super::super::OutputArea {
    pub fn push_tool_result_with_diff(
        &mut self,
        tool_id: &str,
        tool_name: &str,
        result: &str,
        is_error: bool,
        image_note: &str,
    ) {
        self.finish_streaming();

        let header_idx = self.mark_tool_header_done(tool_id, tool_name, is_error);
        let id_tag = Some(tool_id.to_string());
        let mut result_lines = render_tool_result(tool_name, result, is_error, &id_tag);
        append_image_note(image_note, &id_tag, &mut result_lines);
        if tool_name == "TaskListComplete" && result_lines.is_empty() {
            return;
        }
        result_lines.push(OutputLine {
            content: String::new(),
            style: LineStyle::System,
            tool_id: id_tag,
        });
        let insert_at = header_idx
            .map(|start| self.tool_block_end(start, tool_id) + 1)
            .unwrap_or(self.lines.len());
        self.insert_lines_at(insert_at, result_lines);
    }

    fn mark_tool_header_done(
        &mut self,
        tool_id: &str,
        tool_name: &str,
        is_error: bool,
    ) -> Option<usize> {
        let done_icon = if is_error { "✗" } else { "✓" };
        let done_style = if is_error {
            LineStyle::ToolCallError
        } else {
            LineStyle::ToolCallSuccess
        };

        // Phase 1: exact tool_id match
        for (idx, line) in self.lines.iter_mut().enumerate() {
            if matches!(line.style, LineStyle::ToolCallRunning)
                && line.tool_id.as_deref() == Some(tool_id)
            {
                line.content = line.content.replacen('●', done_icon, 1);
                line.style = done_style;
                return Some(idx);
            }
        }

        // Phase 2: match pending placeholder by tool name prefix
        // (e.g., pending:{tool_name}:{index}) — handles orphaned
        // placeholders whose ToolCall event was not processed
        let pending_prefix = format!("pending:{tool_name}:");
        for (idx, line) in self.lines.iter_mut().enumerate().rev() {
            if matches!(line.style, LineStyle::ToolCallRunning)
                && line
                    .tool_id
                    .as_deref()
                    .is_some_and(|id| id.starts_with(&pending_prefix))
            {
                line.content = line.content.replacen('●', done_icon, 1);
                line.style = done_style;
                return Some(idx);
            }
        }

        // Phase 3: last-resort — any ToolCallRunning line
        for (idx, line) in self.lines.iter_mut().enumerate().rev() {
            if matches!(line.style, LineStyle::ToolCallRunning) {
                line.content = line.content.replacen('●', done_icon, 1);
                line.style = done_style;
                return Some(idx);
            }
        }
        None
    }

    fn tool_block_end(&self, start: usize, tool_id: &str) -> usize {
        let mut end = start;
        while end + 1 < self.lines.len() && self.lines[end + 1].tool_id.as_deref() == Some(tool_id)
        {
            end += 1;
        }
        end
    }
}

fn render_tool_result(
    tool_name: &str,
    result: &str,
    is_error: bool,
    id_tag: &Option<String>,
) -> Vec<OutputLine> {
    if is_error {
        return vec![OutputLine {
            content: format!("{INDENT}✗ {result}"),
            style: LineStyle::ToolCallError,
            tool_id: id_tag.clone(),
        }];
    }

    if tool_name == "Edit" && result.contains("---DIFF---\n") {
        return render_edit_diff_result(tool_name, result, id_tag);
    }

    let mut result_lines = render_result_body(tool_name, result, id_tag);
    push_summary_lines(tool_name, result, is_error, id_tag, &mut result_lines);
    result_lines
}

fn render_edit_diff_result(
    tool_name: &str,
    result: &str,
    id_tag: &Option<String>,
) -> Vec<OutputLine> {
    let mut result_lines = Vec::new();
    let parts: Vec<&str> = result.splitn(3, "---DIFF---\n").collect();
    if parts.len() == 3 {
        let summary = parts[0].trim();
        build_diff_lines(parts[1], parts[2], id_tag, &mut result_lines);
        result_lines.push(OutputLine {
            content: format!("{INDENT}✓ {summary}"),
            style: LineStyle::ToolCallSuccess,
            tool_id: id_tag.clone(),
        });
    } else {
        push_summary_lines(tool_name, result, false, id_tag, &mut result_lines);
    }
    result_lines
}

fn render_result_body(tool_name: &str, result: &str, id_tag: &Option<String>) -> Vec<OutputLine> {
    let mut lines = Vec::new();
    if result.trim().is_empty() {
        return lines;
    }

    let (max_lines, result_style) = lookup_display(tool_name)
        .map(|display| (display.result_max_lines(), display.result_style()))
        .unwrap_or((3, LineStyle::System));
    if max_lines == 0 {
        return lines;
    }

    let total = result.lines().count();
    for line in result.lines().take(max_lines) {
        lines.push(OutputLine {
            content: format!("{INDENT}{line}"),
            style: result_style,
            tool_id: id_tag.clone(),
        });
    }
    if total > max_lines {
        lines.push(OutputLine {
            content: format!("{INDENT}... ({} lines omitted)", total - max_lines),
            style: result_style,
            tool_id: id_tag.clone(),
        });
    }
    lines
}

fn push_summary_lines(
    tool_name: &str,
    result: &str,
    is_error: bool,
    id_tag: &Option<String>,
    result_lines: &mut Vec<OutputLine>,
) {
    let summaries = lookup_display(tool_name)
        .map(|display| display.format_result_summary(result, is_error))
        .unwrap_or_else(|| vec![format!("✓ {tool_name} completed")]);
    let style = if is_error {
        LineStyle::ToolCallError
    } else {
        LineStyle::ToolCallSuccess
    };
    for summary in summaries {
        result_lines.push(OutputLine {
            content: format!("{INDENT}{summary}"),
            style,
            tool_id: id_tag.clone(),
        });
    }
}

fn append_image_note(
    image_note: &str,
    id_tag: &Option<String>,
    result_lines: &mut Vec<OutputLine>,
) {
    if !image_note.is_empty() {
        result_lines.push(OutputLine {
            content: image_note.trim().to_string(),
            style: LineStyle::System,
            tool_id: id_tag.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::OutputArea;
    use crate::tui::output_area::LineStyle;

    #[test]
    fn test_push_tool_result_marks_matching_pending_placeholder() {
        let mut output = OutputArea::new();
        output.push_tool_call_start("Bash", 0);
        output.push_tool_call_start("Edit", 1);

        output.push_tool_result_with_diff("edit-1", "Edit", "updated", false, "");

        let bash = output
            .lines
            .iter()
            .find(|line| line.tool_id.as_deref() == Some("pending:Bash:0"))
            .expect("bash placeholder should remain");
        assert_eq!(bash.content, "● Bash...");
        assert!(matches!(bash.style, LineStyle::ToolCallRunning));

        let edit = output
            .lines
            .iter()
            .find(|line| line.tool_id.as_deref() == Some("pending:Edit:1"))
            .expect("edit placeholder should be marked done");
        assert_eq!(edit.content, "✓ Edit...");
        assert!(matches!(edit.style, LineStyle::ToolCallSuccess));
    }

    #[test]
    fn test_push_tool_result_uses_exact_tool_id_before_pending_fallback() {
        let mut output = OutputArea::new();
        output.push_tool_call_start("Edit", 0);
        output.push_tool_call("edit-1", "Edit", r#"{"file_path":"src/lib.rs"}"#);
        output.push_tool_call_start("Edit", 1);

        output.push_tool_result_with_diff("edit-1", "Edit", "updated", false, "");

        let exact = output
            .lines
            .iter()
            .find(|line| line.tool_id.as_deref() == Some("edit-1"))
            .expect("exact tool call should exist");
        assert_eq!(exact.content, "✓ Edit(src/lib.rs)");
        assert!(matches!(exact.style, LineStyle::ToolCallSuccess));

        let pending = output
            .lines
            .iter()
            .find(|line| line.tool_id.as_deref() == Some("pending:Edit:1"))
            .expect("later pending placeholder should remain running");
        assert_eq!(pending.content, "● Edit...");
        assert!(matches!(pending.style, LineStyle::ToolCallRunning));
    }
}
