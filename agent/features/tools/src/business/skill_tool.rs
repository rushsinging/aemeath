use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::skill_ops::Skill;
use share::tool::types::skill::{SkillInput, SkillResult};
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
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::skill(lang))
    }

    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        SkillInput::data_schema()
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
        let args: SkillInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": format!("invalid input: {e}"),
                        "data": {}
                    })
                    .to_string(),
                )
            }
        };

        if args.skill.is_empty() {
            return TypedToolResult::error(
                serde_json::json!({
                    "status": "error",
                    "message": "missing required parameter: skill",
                    "data": {}
                })
                .to_string(),
            );
        }
        let skill_name = args.skill.as_str();

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
        let path = skill.source_path.to_string_lossy().to_string();
        let output = format!("Skill '{}' loaded", skill.name);
        TypedToolResult::success(
            output,
            SkillResult {
                name: skill.name,
                path,
            },
        )
    }
}
