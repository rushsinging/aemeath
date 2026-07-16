use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use context::application::ContextApplicationService;
use context::domain::{
    CalendarDate, ContextAppend, ContextRequest, ContextRequestId, Language, SessionId,
    SessionRevision, SystemPromptSpec, TaskReminderSnapshot,
};
use context::ports::{
    ContextMemorySource, ContextPromptSource, MemoryMaterialization, PromptMaterialization,
    SessionRepository, SessionSnapshot,
};
use provider::api::ReasoningLevel;
use sdk::RunId;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;

struct Session;
#[async_trait]
impl SessionRepository for Session {
    async fn snapshot(&self, _session_id: &SessionId) -> Result<SessionSnapshot, String> {
        Ok(SessionSnapshot {
            revision: SessionRevision::new(0),
            messages: vec![],
            active_summary: None,
        })
    }

    async fn append_finalized(
        &self,
        _append: &ContextAppend,
    ) -> Result<context::domain::AppendReceipt, context::domain::ContextAppendError> {
        unreachable!()
    }

    async fn commit_compaction(
        &self,
        _request: &context::domain::CompactRequest,
    ) -> Result<context::domain::CompactOutcome, context::domain::ContextPortError> {
        unreachable!()
    }
}

struct FailingPrompt;
#[async_trait]
impl ContextPromptSource for FailingPrompt {
    async fn materialize(
        &self,
        _request: &ContextRequest,
    ) -> Result<PromptMaterialization, String> {
        Err("guidance unavailable".into())
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
        pending_messages: vec![],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        current_date: CalendarDate::new("2026-07-15"),
        task_reminder: TaskReminderSnapshot::default(),
        language: Language::new("zh"),
        agent_roles: Default::default(),
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

#[tokio::test]
async fn prompt_failure_is_typed_and_stops_before_memory_materialization() {
    use context::ports::ContextPort;

    let memory_calls = Arc::new(AtomicUsize::new(0));
    let service = ContextApplicationService::new(
        Arc::new(Session),
        Arc::new(FailingPrompt),
        Arc::new(CountingMemory(memory_calls.clone())),
    );

    assert!(matches!(
        service.build_window(&request()).await,
        Err(context::domain::ContextPortError::PromptMaterialization(message))
            if message == "guidance unavailable"
    ));
    assert_eq!(memory_calls.load(Ordering::SeqCst), 0);
}
