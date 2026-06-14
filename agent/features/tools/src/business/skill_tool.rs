use crate::api::{Tool, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::skill_ops::Skill;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SkillTool {
    pub skills: Arc<Mutex<HashMap<String, Skill>>>,
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "Skill"
    }

    fn description(&self) -> &str {
        "Execute a skill within the conversation. Skills are reusable prompt templates loaded from .claude/skills/ directories.\n\nUsage:\n- Use skill name to invoke (e.g., skill: \"commit\")\n- Optional args are passed to the skill content\n- Available skills are listed in system messages"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "The skill name to execute"
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments for the skill"
                }
            },
            "required": ["skill"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, _ctx: &ToolExecutionContext) -> ToolResult {
        let skill_name = match input.get("skill").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolResult::error(serde_json::json!({
                "status": "error",
                "message": "missing required parameter: skill",
                "data": {}
            }).to_string()),
        };

        let args = input.get("args").and_then(|v| v.as_str()).unwrap_or("");

        let skills = self.skills.lock().await;
        let skill = match skills.get(skill_name) {
            Some(s) => s.clone(),
            None => {
                let available: Vec<&str> = skills.keys().map(|s| s.as_str()).collect();
                return ToolResult::error(serde_json::json!({
                    "status": "error",
                    "message": format!("skill '{}' not found", skill_name),
                    "data": {
                        "available_skills": available
                    }
                }).to_string());
            }
        };
        drop(skills);

        // Skill content is materialized by prompt domain before registration.
        let mut content = skill.content.clone();
        if !args.is_empty() {
            content = format!("{content}\n\nArguments: {args}");
        }

        ToolResult::success(serde_json::json!({
            "status": "success",
            "message": format!("Skill '{}' loaded", skill.name),
            "data": {
                "skill": skill.name,
                "content": content
            }
        }).to_string())
    }
}
