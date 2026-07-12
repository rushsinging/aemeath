//! Typed result for the `agent` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `agent` tool (sub-agent dispatch).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AgentResult {
    pub output: String,
}

/// Typed input for the `agent` tool (sub-agent dispatch).
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// 字段标识符即 JSON property 名（build.rs 使用标识符，忽略 serde rename）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentInput {
    /// The task for the agent to perform
    pub prompt: String,
    /// A short (3-5 word) description of the task
    pub description: String,
    /// Agent role name defined in config (e.g. 'coder', 'reviewer'). Resolves to the model and settings configured for that role.
    pub role: Option<String>,
    /// Maximum number of tool-call rounds (max 1000)
    pub max_turns: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_input() {
        let json = serde_json::json!({"prompt": "p", "description": "d"});
        let input: AgentInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.prompt, "p");
        assert_eq!(input.description, "d");
        assert!(input.role.is_none());
        assert!(input.max_turns.is_none());
    }

    #[test]
    fn full_input_with_role_and_turns() {
        let json = serde_json::json!({"prompt": "p", "description": "d", "role": "coder", "max_turns": 50});
        let input: AgentInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.role.as_deref(), Some("coder"));
        assert_eq!(input.max_turns, Some(50));
    }
}
