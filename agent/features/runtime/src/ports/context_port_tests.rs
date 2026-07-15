use std::collections::HashMap;

use async_trait::async_trait;
use provider::api::ReasoningLevel;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;

use super::*;

struct FakeContextPort;

fn request() -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: sdk::RunId::new("run"),
        pending_messages: vec![],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        current_date: CalendarDate::new("2026-07-15"),
        task_reminder: TaskReminderSnapshot::default(),
        language: Language::new("zh"),
        agent_roles: HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_input_tokens: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
        prev_system_tokens: None,
        prev_tool_schema_tokens: None,
    }
}

#[async_trait]
impl ContextPort for FakeContextPort {
    async fn build_window(
        &self,
        request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError> {
        Ok(ContextWindow {
            system_blocks: vec![],
            messages: request.pending_messages.clone(),
            tool_schemas: request.tool_schemas.clone(),
            token_estimation: TokenBudget::default(),
            compaction_decision: self.needs_compaction(request).await?,
        })
    }

    async fn needs_compaction(
        &self,
        _request: &ContextRequest,
    ) -> Result<CompactionDecision, ContextPortError> {
        Ok(CompactionDecision {
            needed: false,
            urgency: Urgency::None,
            estimated_tokens: 0,
            threshold: 1,
            reason: DecisionReason::Heuristic,
        })
    }

    async fn compact(&self, _request: &CompactRequest) -> Result<CompactOutcome, ContextPortError> {
        Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection))
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError> {
        Ok(AppendReceipt {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(1),
            fingerprint: append.fingerprint.clone(),
        })
    }
}

#[tokio::test]
async fn runtime_fake_compiles_against_context_owned_port() {
    let request = request();
    let window = FakeContextPort.build_window(&request).await.unwrap();
    assert!(window.messages.is_empty());
}
