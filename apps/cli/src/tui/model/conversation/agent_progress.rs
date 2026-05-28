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
