use share::ids::SessionId;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::ContextApplicationService;
use crate::{
    ContextAppendData, ContextRequestData, ContextRequestId, Language, SessionRevision,
    SystemPromptSpecData,
};
use crate::{
    ContextMemorySource, ContextPromptSource, MemoryMaterialization, PromptMaterialization,
    SessionRepository, SessionSnapshot,
};
use async_trait::async_trait;
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
        _append: &ContextAppendData,
    ) -> Result<crate::AppendReceiptData, share::error::DomainError> {
        unreachable!()
    }

    async fn commit_compaction(
        &self,
        _request: &crate::CompactRequestData,
    ) -> Result<crate::CompactOutcome, share::error::DomainError> {
        unreachable!()
    }

    async fn commit_manual_compaction(
        &self,
        _request: &crate::ManualCompactRequestData,
    ) -> Result<crate::CompactOutcome, share::error::DomainError> {
        Ok(crate::CompactOutcome::Committed(crate::CompactResult {
            summary: "manual".into(),
            recent_messages: vec![],
            source_revision: SessionRevision::new(1),
            quality: crate::CompactSummaryQuality::LocalOnly,
        }))
    }

    async fn clear(&self, _session_id: &SessionId) -> Result<(), share::error::DomainError> {
        Ok(())
    }
}

struct FailingPrompt;
#[async_trait]
impl ContextPromptSource for FailingPrompt {
    async fn materialize(
        &self,
        _request: &ContextRequestData,
    ) -> Result<PromptMaterialization, String> {
        Err("guidance unavailable".to_string())
    }
}

struct CountingMemory(Arc<AtomicUsize>);
#[async_trait]
impl ContextMemorySource for CountingMemory {
    async fn materialize(
        &self,
        _request: &ContextRequestData,
    ) -> Result<MemoryMaterialization, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(MemoryMaterialization {
            blocks: vec![],
            revision: 1,
        })
    }
}

fn request() -> ContextRequestData {
    ContextRequestData {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: share::ids::RunStepId::new("step"),
        pending_messages: vec![],
        system_prompt: SystemPromptSpecData::new("system"),
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
    use crate::ContextPort;

    let memory_calls = Arc::new(AtomicUsize::new(0));
    let service = ContextApplicationService::new(
        Arc::new(Session),
        Arc::new(FailingPrompt),
        Arc::new(CountingMemory(memory_calls.clone())),
    );

    assert!(matches!(
        service.build_window(&request()).await,
        Err(err) if err.message() == "Prompt 物化失败：guidance unavailable"
    ));
    assert_eq!(memory_calls.load(Ordering::SeqCst), 0);
}
