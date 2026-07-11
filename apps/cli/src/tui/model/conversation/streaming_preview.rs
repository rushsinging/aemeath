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
    committed_lines: Vec<String>,
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

    pub fn push_chunk(&mut self, chunk: &str) {
        for segment in chunk.split_inclusive('\n') {
            if let Some(without_newline) = segment.strip_suffix('\n') {
                self.partial_line.push_str(without_newline);
                self.commit_partial_line();
            } else {
                self.partial_line.push_str(segment);
            }
        }
    }

    pub fn display_text(&self) -> String {
        self.display_lines().join("\n")
    }

    pub fn display_lines(&self) -> Vec<String> {
        let max_lines = self.policy.max_lines.max(1);
        let mut lines = self.committed_lines.clone();
        if self.policy.include_partial_line && !self.partial_line.is_empty() {
            lines.push(self.partial_line.clone());
        }
        let selected = if self.policy.tail_mode && lines.len() > max_lines {
            lines[lines.len() - max_lines..].to_vec()
        } else {
            lines.into_iter().take(max_lines).collect()
        };
        selected
            .into_iter()
            .map(|line| truncate_chars(&line, self.policy.max_line_chars))
            .collect()
    }

    fn commit_partial_line(&mut self) {
        self.committed_lines
            .push(std::mem::take(&mut self.partial_line));
        self.trim_committed_lines();
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
    fn commits_lines_only_after_newline_and_keeps_partial_preview() {
        let mut buffer = ToolStreamingPreviewBuffer::new(policy());
        buffer.push_chunk("abc");
        assert_eq!(buffer.display_lines(), vec!["abc"]);

        buffer.push_chunk("def\nnext");
        assert_eq!(buffer.display_lines(), vec!["abcdef", "next"]);
    }

    #[test]
    fn tail_mode_keeps_last_max_lines() {
        let mut buffer = ToolStreamingPreviewBuffer::new(policy());
        buffer.push_chunk("a\nb\nc\nd\n");
        assert_eq!(buffer.display_lines(), vec!["b", "c", "d"]);
    }

    #[test]
    fn truncates_long_lines() {
        let mut buffer = ToolStreamingPreviewBuffer::new(policy());
        buffer.push_chunk("1234567890\nabcdefghi");
        assert_eq!(buffer.display_lines(), vec!["1234567…", "abcdefg…"]);
    }
}
