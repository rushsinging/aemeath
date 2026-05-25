use super::effort_from_thinking_tokens;

#[derive(Debug, Clone, PartialEq)]
pub enum ReasoningConfig {
    Bool(bool),
    Object(serde_json::Value),
    ThinkingBudget(u32),
}

impl ReasoningConfig {
    pub(super) fn as_effort(&self) -> Option<String> {
        match self {
            Self::Object(value) => value
                .get("effort")
                .or_else(|| value.get("reasoning_effort"))
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned),
            Self::ThinkingBudget(tokens) => Some(effort_from_thinking_tokens(*tokens).to_string()),
            Self::Bool(_) => None,
        }
    }
}
