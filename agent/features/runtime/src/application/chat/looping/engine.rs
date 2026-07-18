//! Runtime-local approval compatibility gate.
//!
//! Tool safety preconditions belong to Tool/Workspace. This gate only preserves
//! the legacy auto-approval decision until #917/#918 replace it with PolicyPort.

use serde_json::Value;
use tools::{PolicyDecision, Tool};

/// Compatibility approval gate; it never rewrites tool input.
pub struct PolicyEngine {
    allow_all: bool,
}

impl PolicyEngine {
    pub fn new(allow_all: bool) -> Self {
        Self { allow_all }
    }

    pub fn evaluate(&self, input: &Value, tool: Option<&dyn Tool>) -> PolicyDecision {
        let approved = match tool {
            Some(tool) => self.allow_all || tool.is_read_only() || tool.is_input_safe(input),
            None => self.allow_all,
        };

        if approved {
            PolicyDecision::Allow(input.clone())
        } else {
            PolicyDecision::Deny {
                reason: "This tool requires user confirmation.".into(),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeniedCall {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn approval_gate_preserves_input_without_path_rewrite() {
        let input = json!({"file_path": "../outside.rs"});

        let decision = PolicyEngine::new(true).evaluate(&input, None);

        assert!(matches!(decision, PolicyDecision::Allow(value) if value == input));
    }
}
