#[derive(Clone, Debug, PartialEq)]
pub struct SubRunActivityEntry {
    pub agent_id: String,
    pub run_id: String,
    pub parent_run_id: String,
    pub spawned_by_tool_call_id: String,
    pub sequence: u64,
    pub sequence_index: u32,
    pub kind: crate::tui::adapter::tui_runtime_event::TuiSubRunActivityKind,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AgentActivityKind {
    Message,
    ToolCall,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AgentActivityLine {
    pub kind: AgentActivityKind,
    pub content: String,
}

impl AgentActivityLine {
    pub fn message(content: impl Into<String>) -> Self {
        Self {
            kind: AgentActivityKind::Message,
            content: content.into(),
        }
    }

    pub fn tool_call(content: impl Into<String>) -> Self {
        Self {
            kind: AgentActivityKind::ToolCall,
            content: content.into(),
        }
    }

    pub fn contains(&self, pattern: &str) -> bool {
        self.content.contains(pattern)
    }
}

impl From<&str> for AgentActivityLine {
    fn from(content: &str) -> Self {
        Self::message(content)
    }
}

impl From<String> for AgentActivityLine {
    fn from(content: String) -> Self {
        Self::message(content)
    }
}

impl PartialEq<&str> for AgentActivityLine {
    fn eq(&self, other: &&str) -> bool {
        self.content == *other
    }
}

impl PartialEq<String> for AgentActivityLine {
    fn eq(&self, other: &String) -> bool {
        self.content == *other
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentProgressEntry {
    pub tool_id: String,
    pub message: String,
}

impl AgentProgressEntry {
    pub fn new(tool_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            tool_id: tool_id.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_progress_stores_tool_id() {
        let progress = AgentProgressEntry::new("tool-1", "working");
        assert_eq!(progress.tool_id, "tool-1");
    }

    #[test]
    fn test_agent_progress_stores_message() {
        let progress = AgentProgressEntry::new("tool-1", "working");
        assert_eq!(progress.message, "working");
    }

    #[test]
    fn test_agent_progress_allows_empty_message() {
        let progress = AgentProgressEntry::new("tool-1", "");
        assert_eq!(progress.message, "");
    }
}
