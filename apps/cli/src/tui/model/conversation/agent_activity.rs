#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubRunActivityWatermark {
    pub agent_id: String,
    pub run_id: String,
    pub sequence: u64,
    pub sequence_index: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AgentActivityKind {
    Message,
    ToolCall,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentActivityContent {
    Text(String),
    ToolCall {
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentActivityLine {
    pub kind: AgentActivityKind,
    pub content: AgentActivityContent,
}

impl AgentActivityLine {
    pub fn message(content: impl Into<String>) -> Self {
        Self {
            kind: AgentActivityKind::Message,
            content: AgentActivityContent::Text(content.into()),
        }
    }

    pub fn tool_call(name: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            kind: AgentActivityKind::ToolCall,
            content: AgentActivityContent::ToolCall {
                name: name.into(),
                input,
            },
        }
    }

    #[cfg(test)]
    pub fn contains(&self, pattern: &str) -> bool {
        match &self.content {
            AgentActivityContent::Text(content) => content.contains(pattern),
            AgentActivityContent::ToolCall { name, input } => {
                name.contains(pattern) || input.to_string().contains(pattern)
            }
        }
    }

    #[cfg(test)]
    pub fn text(&self) -> Option<&str> {
        match &self.content {
            AgentActivityContent::Text(content) => Some(content),
            AgentActivityContent::ToolCall { .. } => None,
        }
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
        matches!(&self.content, AgentActivityContent::Text(content) if content == *other)
    }
}

impl PartialEq<String> for AgentActivityLine {
    fn eq(&self, other: &String) -> bool {
        matches!(&self.content, AgentActivityContent::Text(content) if content == other)
    }
}
