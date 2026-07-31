use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::{
    CancellationDeclaration, SkillError, SkillLoadDecision, SkillLoadMutation, SkillLoadPort,
    SkillLoadQuery, SkillLoadStateError, ToolExecutionContext, TypedTool, TypedToolResult,
};

#[derive(Debug, Deserialize)]
struct DynamicSkillToolInput {
    skill: String,
}

#[derive(Debug, Serialize)]
pub struct SkillLoadResult {
    pub name: String,
    pub revision: String,
}

/// Stable Tool entry that dynamically loads one Skill body at invocation time.
pub struct SkillTool {
    loader: Arc<dyn SkillLoadPort>,
}

impl SkillTool {
    pub fn new(loader: Arc<dyn SkillLoadPort>) -> Self {
        Self { loader }
    }
}

#[async_trait]
impl TypedTool for SkillTool {
    type Output = SkillLoadResult;

    fn name(&self) -> &str {
        "Skill"
    }

    fn description(&self) -> &str {
        "Load one available Skill by identity when its instructions are needed."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill": { "type": "string", "minLength": 1 }
            },
            "required": ["skill"],
            "additionalProperties": false
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn cancellation(&self) -> CancellationDeclaration {
        CancellationDeclaration::Cooperative
    }

    fn is_input_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let args: DynamicSkillToolInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("Skill 参数无效: {error}")),
        };
        let snapshot = ctx.skill_query();
        let query = match SkillLoadQuery::new(
            args.skill,
            ctx.workspace_read().current_workspace_root(),
            snapshot.extra_dirs.clone(),
            snapshot.available_tools.clone(),
        ) {
            Ok(query) => query,
            Err(error) => return TypedToolResult::error(safe_error(error)),
        };
        match self.loader.load(query).await {
            Ok(loaded) => {
                let Some(session_id) = ctx.parent_session_id() else {
                    return TypedToolResult::error("Skill 加载状态未绑定 Session".to_string());
                };
                let Some(scope) = ctx.skill_load_scope().cloned() else {
                    return TypedToolResult::error("Skill 加载状态未绑定 Agent 作用域".to_string());
                };
                let Some(state) = ctx.skill_load_state() else {
                    return TypedToolResult::error("Skill 加载状态服务不可用".to_string());
                };
                let mutation = match SkillLoadMutation::new(
                    session_id,
                    scope,
                    loaded.name(),
                    loaded.revision(),
                ) {
                    Ok(mutation) => mutation,
                    Err(error) => return TypedToolResult::error(safe_state_error(error)),
                };
                match state.compare_and_record(mutation).await {
                    Ok(decision) => {
                        let content = match decision {
                            SkillLoadDecision::Fresh | SkillLoadDecision::Updated => {
                                loaded.content().to_string()
                            }
                            SkillLoadDecision::AlreadyLoaded => format!(
                                "Skill {} 已加载，内容未更新（revision: {}）。请继续使用已有指令。",
                                loaded.name(),
                                loaded.revision()
                            ),
                        };
                        TypedToolResult::success(
                            content,
                            SkillLoadResult {
                                name: loaded.name().to_string(),
                                revision: loaded.revision().to_string(),
                            },
                        )
                    }
                    Err(error) => TypedToolResult::error(safe_state_error(error)),
                }
            }
            Err(error) => TypedToolResult::error(safe_error(error)),
        }
    }
}

fn safe_state_error(error: SkillLoadStateError) -> String {
    match error {
        SkillLoadStateError::InvalidSessionId
        | SkillLoadStateError::InvalidInstanceId
        | SkillLoadStateError::InvalidSkillName
        | SkillLoadStateError::InvalidRevision => "Skill 加载状态无效".to_string(),
        SkillLoadStateError::SessionNotFound(_) => "Skill 加载 Session 不存在".to_string(),
        SkillLoadStateError::Storage(_) => "保存 Skill 加载状态失败".to_string(),
    }
}

fn safe_error(error: SkillError) -> String {
    match error {
        SkillError::InvalidIdentity { .. } => "Skill identity 无效".to_string(),
        SkillError::NotFound { identity } => format!("Skill 不存在或当前不可用: {identity}"),
        SkillError::ReadFailed { .. } => "读取 Skill 失败".to_string(),
        SkillError::ParseFailed { .. } => "解析 Skill 失败".to_string(),
    }
}
