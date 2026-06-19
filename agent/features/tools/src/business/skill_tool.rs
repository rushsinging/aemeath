use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::skill_ops::Skill;
use share::tool::types::skill::SkillResult;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SkillTool {
    pub skills: Arc<Mutex<HashMap<String, Skill>>>,
}

#[async_trait]
impl TypedTool for SkillTool {
    type Output = SkillResult;
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
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        SkillResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<SkillResult> {
        let skill_name = match input.get("skill").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": "missing required parameter: skill",
                        "data": {}
                    })
                    .to_string(),
                )
            }
        };

        let skills = self.skills.lock().await;
        let skill = match skills.get(skill_name) {
            Some(s) => s.clone(),
            None => {
                let available: Vec<&str> = skills.keys().map(|s| s.as_str()).collect();
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": format!("skill '{}' not found", skill_name),
                        "data": {
                            "available_skills": available
                        }
                    })
                    .to_string(),
                );
            }
        };
        drop(skills);

        // Skill content is materialized by prompt domain before registration.
        TypedToolResult::success_msg(
            serde_json::json!({
                "status": "success",
                "message": format!("Skill '{}' loaded", skill.name),
                "data": serde_json::to_value(SkillResult {
                    name: skill.name,
                    path: skill.source_path.to_string_lossy().to_string()
                }).unwrap()
            })
            .to_string(),
        )
    }
}
