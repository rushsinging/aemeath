use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use context::ContextApplicationService;
use context::{
    ContextAppend, ContextRequest, ContextRequestId, Language, SessionId, SessionRevision,
    SystemPromptSpec,
};
use context::{
    ContextMemorySource, ContextPromptSource, MemoryMaterialization, PromptMaterialization,
    SessionRepository, SessionSnapshot,
};
use sdk::RunId;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::reasoning::ReasoningLevel;

struct Session;
#[async_trait]
impl SessionRepository for Session {
    async fn snapshot(&self, _session_id: &SessionId) -> Result<SessionSnapshot, String> {
        Ok(SessionSnapshot {
            revision: SessionRevision::new(0),
            messages: vec![].into(),
            structured_history: None,
            active_summary: None,
        })
    }

    async fn append_finalized(
        &self,
        _append: &ContextAppend,
    ) -> Result<context::AppendReceipt, context::ContextAppendError> {
        unreachable!()
    }

    async fn commit_compaction(
        &self,
        _request: &context::CompactRequest,
    ) -> Result<context::CompactOutcome, context::ContextPortError> {
        unreachable!()
    }

    async fn commit_manual_compaction(
        &self,
        _request: &context::ManualCompactRequest,
    ) -> Result<context::CompactOutcome, context::ContextPortError> {
        Ok(context::CompactOutcome::Committed(context::CompactResult {
            summary: "manual".into(),
            recent_messages: vec![],
            source_revision: SessionRevision::new(1),
            quality: context::CompactSummaryQuality::LocalOnly,
        }))
    }

    async fn clear(&self, _session_id: &SessionId) -> Result<(), context::ContextPortError> {
        Ok(())
    }
}

struct FailingPrompt;
#[async_trait]
impl ContextPromptSource for FailingPrompt {
    async fn materialize(
        &self,
        _request: &ContextRequest,
    ) -> Result<PromptMaterialization, context::PromptMaterializationError> {
        Err(context::PromptMaterializationError::Baseline(
            "guidance unavailable".into(),
        ))
    }
}

struct CountingMemory(Arc<AtomicUsize>);
#[async_trait]
impl ContextMemorySource for CountingMemory {
    async fn materialize(
        &self,
        _request: &ContextRequest,
    ) -> Result<MemoryMaterialization, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(MemoryMaterialization {
            blocks: vec![],
            revision: 1,
        })
    }
}

fn request() -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: context::RunStepId::new("step"),
        pending_messages: vec![],
        invocation_reminders: vec![],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::new("zh"),
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        heuristic_calibration: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

#[tokio::test]
async fn prompt_failure_is_typed_and_stops_before_memory_materialization() {
    use context::ContextPort;

    let memory_calls = Arc::new(AtomicUsize::new(0));
    let service = ContextApplicationService::new(
        Arc::new(Session),
        Arc::new(FailingPrompt),
        Arc::new(CountingMemory(memory_calls.clone())),
    );

    assert!(matches!(
        service.build_window(&request()).await,
        Err(context::ContextPortError::PromptMaterialization(
            context::PromptMaterializationError::Baseline(msg)
        )) if msg == "guidance unavailable"
    ));
    assert_eq!(memory_calls.load(Ordering::SeqCst), 0);
}
