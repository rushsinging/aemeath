use std::collections::HashMap;

use share::message::Message;

use crate::application::run::config::RunConfigSnapshot;
use crate::application::run::context::RuntimeContext;
use crate::ports::{
    ContextRequest, ContextRequestId, Language, ModelToolSchema, RunStepId, SessionId,
    SystemPromptSpec,
};

/// Frozen values required to build one Step's Context request.
pub(crate) struct ContextRequestSource<'a> {
    pub runtime_context: &'a RuntimeContext,
    pub session_id: &'a str,
    pub system_prompt: &'a str,
    pub model_id: &'a str,
    pub language: &'a str,
    pub agent_roles: HashMap<String, share::config::AgentRoleConfig>,
    pub config: &'a RunConfigSnapshot,
    pub context_size: usize,
    pub max_output_tokens: usize,
    pub raw_tool_schemas: Vec<serde_json::Value>,
}

/// Role-neutral owner of ContextRequest assembly.
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
    ) -> ContextRequest {
        let tool_schemas = self
            .source
            .raw_tool_schemas
            .iter()
            .filter_map(|schema| {
                Some(ModelToolSchema {
                    name: schema.get("name")?.as_str()?.to_string(),
                    description: schema.get("description")?.as_str()?.to_string(),
                    input_schema: schema.get("input_schema")?.clone(),
                })
            })
            .collect();
        ContextRequest {
            session_id: SessionId::new(self.source.session_id),
            request_id: ContextRequestId::new(uuid::Uuid::now_v7().to_string()),
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            pending_messages,
            system_prompt: SystemPromptSpec::new(self.source.system_prompt),
            model_id: self.source.model_id.to_string(),
            effective_reasoning: *self
                .source
                .runtime_context
                .reasoning_ref()
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            language: Language::new(self.source.language),
            agent_roles: self.source.agent_roles.clone(),
            config_snapshot: self.source.config.config().clone(),
            context_size: self.source.context_size,
            max_output_tokens: self.source.max_output_tokens,
            last_api_total_tokens: self.source.runtime_context.usage().get(),
            tool_schemas,
            tool_schema_tokens: context::compact::estimate_tool_schemas_tokens(
                &self.source.raw_tool_schemas,
            ),
        }
    }
}
