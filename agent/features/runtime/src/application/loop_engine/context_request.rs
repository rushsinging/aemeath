use std::collections::HashMap;

use share::message::Message;

use crate::application::run::config::RunConfigSnapshot;
use crate::application::run::context::RuntimeContext;
use crate::ports::{ContextRequestData, InteractionRequestId, Language, RunStepId, SessionId};

/// Frozen values required to build one Step's Context request.
pub(crate) struct ContextRequestSource<'a> {
    pub runtime_context: &'a RuntimeContext,
    pub session_id: &'a str,
    /// 工作区根与 git 事实：Context 组装系统提示所需，Runtime 启动时探测一次。
    pub workspace_root: &'a std::path::Path,
    pub workspace_is_git: bool,
    pub model_id: &'a str,
    pub language: &'a str,
    pub agent_roles: HashMap<String, share::config::AgentRoleDefinition>,
    pub config: &'a RunConfigSnapshot,
    pub context_size: usize,
    pub max_output_tokens: usize,
    pub raw_tool_schemas: Vec<serde_json::Value>,
}

/// Role-neutral owner of ContextRequestData assembly.
pub(crate) struct ContextRequestCoordinator<'a> {
    source: ContextRequestSource<'a>,
}

impl<'a> ContextRequestCoordinator<'a> {
    pub(crate) fn new(source: ContextRequestSource<'a>) -> Self {
        Self { source }
    }

    pub(crate) fn build_request(
        &self,
        run_id: &sdk::RunId,
        step_id: &RunStepId,
        pending_messages: Vec<Message>,
    ) -> ContextRequestData {
        let tool_schemas: Vec<tools::ToolDefinition> = self
            .source
            .raw_tool_schemas
            .iter()
            .filter_map(tools::ToolDefinition::from_wire)
            .collect();
        ContextRequestData {
            session_id: SessionId::new(self.source.session_id),
            request_id: InteractionRequestId::new(share::ids::new_typed_id("irq")),
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            pending_messages,
            workspace_root: self.source.workspace_root.to_path_buf(),
            workspace_is_git: self.source.workspace_is_git,
            model_id: self.source.model_id.to_string(),
            effective_reasoning: *self
                .source
                .runtime_context
                .reasoning_ref()
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            language: Language::from_config(self.source.language),
            agent_roles: self.source.agent_roles.clone(),
            config_snapshot: self.source.config.config().clone(),
            context_size: self.source.context_size,
            max_output_tokens: self.source.max_output_tokens,
            last_api_total_tokens: self.source.runtime_context.usage().get(),
            heuristic_calibration: Some(self.source.runtime_context.usage().calibration_factor()),
            tool_schema_tokens: context::compact::estimate_tool_schemas_tokens(&tool_schemas),
            tool_schemas,
        }
    }
}
