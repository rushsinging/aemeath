use share::ids::{InteractionRequestId, SessionId};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::ContextApplicationService;
use crate::{ContextAppendData, ContextRequestData, Language, SessionRevision};
use crate::{
    ContextMemorySource, ContextPromptSource, MemoryMaterialization, PromptMaterialization,
    SessionItem, SessionRepository,
};
use async_trait::async_trait;
use sdk::RunId;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::reasoning::ReasoningLevel;

struct Session;
#[async_trait]
impl SessionRepository for Session {
    async fn snapshot(&self, _session_id: &SessionId) -> Result<SessionItem, String> {
        Ok(SessionItem {
            revision: SessionRevision::new(0),
            messages: vec![].into(),
            structured_history: None,
            active_summary: None,
        })
    }

    async fn append_finalized(
        &self,
        _append: &ContextAppendData,
    ) -> Result<crate::AppendReceiptData, crate::ContextAppendError> {
        unreachable!()
    }

    async fn commit_compaction(
        &self,
        _request: &crate::CompactRequestData,
    ) -> Result<crate::CompactOutcome, crate::ContextPortError> {
        unreachable!()
    }

    async fn clear(&self, _session_id: &SessionId) -> Result<(), crate::ContextPortError> {
        Ok(())
    }
}

struct FailingPrompt;
#[async_trait]
impl ContextPromptSource for FailingPrompt {
    async fn materialize(
        &self,
        _request: &ContextRequestData,
    ) -> Result<PromptMaterialization, crate::PromptMaterializationError> {
        Err(crate::PromptMaterializationError::Baseline(
            "guidance unavailable".into(),
        ))
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
        request_id: InteractionRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: share::ids::RunStepId::new("step"),
        pending_messages: vec![],
        workspace_root: std::path::PathBuf::from("/tmp/workspace"),
        workspace_is_git: false,
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::Zh,
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
    use crate::ContextWindowPort;

    let memory_calls = Arc::new(AtomicUsize::new(0));
    let service = ContextApplicationService::new(
        Arc::new(Session),
        Arc::new(FailingPrompt),
        Arc::new(CountingMemory(memory_calls.clone())),
    );

    assert!(matches!(
        service.build_window(&request()).await,
        Err(crate::ContextPortError::PromptMaterialization(
            crate::PromptMaterializationError::Baseline(msg)
        )) if msg == "guidance unavailable"
    ));
    assert_eq!(memory_calls.load(Ordering::SeqCst), 0);
}
