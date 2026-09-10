use crate::tui::model::conversation::agent_activity::AgentActivityLine;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolStreamingPreviewPolicy {
    pub max_lines: usize,
    pub tail_mode: bool,
    pub max_line_chars: usize,
    pub include_partial_line: bool,
}

impl ToolStreamingPreviewPolicy {
    pub const fn new(max_lines: usize, tail_mode: bool, max_line_chars: usize) -> Self {
        Self {
            max_lines,
            tail_mode,
            max_line_chars,
            include_partial_line: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolStreamingPreviewBuffer {
    policy: ToolStreamingPreviewPolicy,
    committed_lines: Vec<AgentActivityLine>,
    partial_line: String,
}

impl ToolStreamingPreviewBuffer {
    pub fn new(policy: ToolStreamingPreviewPolicy) -> Self {
        Self {
            policy,
            committed_lines: Vec::new(),
            partial_line: String::new(),
        }
    }

    pub fn push_activity(&mut self, activity: AgentActivityLine) {
        let super::agent_activity::AgentActivityContent::Text(content) = &activity.content else {
            self.committed_lines.push(activity);
            self.trim_committed_lines();
            return;
        };
        let mut lines = content.lines();
        if let Some(first_line) = lines.next() {
            self.committed_lines
                .push(AgentActivityLine::message(first_line));
            for line in lines {
                self.committed_lines.push(AgentActivityLine::message(line));
            }
            self.trim_committed_lines();
        }
    }

    pub fn display_lines(&self) -> Vec<AgentActivityLine> {
        let max_lines = self.policy.max_lines.max(1);
        let mut lines = self.committed_lines.clone();
        if self.policy.include_partial_line && !self.partial_line.is_empty() {
            lines.push(AgentActivityLine::message(self.partial_line.clone()));
        }
        let selected = if self.policy.tail_mode && lines.len() > max_lines {
            lines[lines.len() - max_lines..].to_vec()
        } else {
            lines.into_iter().take(max_lines).collect()
        };
        selected
            .into_iter()
            .map(|mut activity| {
                if let super::agent_activity::AgentActivityContent::Text(content) =
                    &mut activity.content
                {
                    *content = truncate_chars(content, self.policy.max_line_chars);
                }
                activity
            })
            .collect()
    }

    fn trim_committed_lines(&mut self) {
        let retain = self.policy.max_lines.max(1);
        if self.policy.tail_mode && self.committed_lines.len() > retain {
            let drop_count = self.committed_lines.len() - retain;
            self.committed_lines.drain(0..drop_count);
        }
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let mut output: String = value.chars().take(keep).collect();
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> ToolStreamingPreviewPolicy {
        ToolStreamingPreviewPolicy::new(3, true, 8)
    }

    #[test]
    fn tail_mode_keeps_last_max_lines() {
        let mut buffer = ToolStreamingPreviewBuffer::new(policy());
        for text in ["a", "b", "c", "d"] {
            buffer.push_activity(AgentActivityLine::message(text.to_string()));
        }
        let lines = buffer.display_lines();
        let texts: Vec<&str> = lines.iter().map(|l| l.text().unwrap_or_default()).collect();
        assert_eq!(texts, vec!["b", "c", "d"]);
    }

    #[test]
    fn truncates_long_lines() {
        let mut buffer = ToolStreamingPreviewBuffer::new(policy());
        buffer.push_activity(AgentActivityLine::message("1234567890".to_string()));
        buffer.push_activity(AgentActivityLine::message("abcdefghi".to_string()));
        let lines = buffer.display_lines();
        let texts: Vec<&str> = lines.iter().map(|l| l.text().unwrap_or_default()).collect();
        assert_eq!(texts, vec!["1234567…", "abcdefg…"]);
    }
}
