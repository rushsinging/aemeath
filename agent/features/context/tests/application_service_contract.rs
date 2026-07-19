use std::sync::Arc;

use async_trait::async_trait;
use context::application::ContextApplicationService;
use context::domain::{
    CalendarDate, ContextAppend, ContextRequest, ContextRequestId, FinalizeCause, Language,
    RunStepId, SessionId, SessionRevision, SystemBlock, SystemPromptSpec, TaskReminderSnapshot,
};
use context::ports::{
    ContextMemorySource, ContextPort, ContextPromptSource, MemoryMaterialization,
    PromptMaterialization, SessionRepository, SessionSnapshot,
};
use provider::ReasoningLevel;
use sdk::RunId;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::Message;

struct FakeSession;

#[async_trait]
impl SessionRepository for FakeSession {
    async fn snapshot(&self, _session_id: &SessionId) -> Result<SessionSnapshot, String> {
        Ok(SessionSnapshot {
            revision: SessionRevision::new(2),
            messages: vec![Message::user("history")],
            active_summary: Some("summary".into()),
        })
    }

    async fn append_finalized(
        &self,
        append: &ContextAppend,
    ) -> Result<context::domain::AppendReceipt, context::domain::ContextAppendError> {
        Ok(context::domain::AppendReceipt {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(3),
            fingerprint: append.fingerprint.clone(),
        })
    }

    async fn commit_compaction(
        &self,
        _request: &context::domain::CompactRequest,
    ) -> Result<context::domain::CompactOutcome, context::domain::ContextPortError> {
        Ok(context::domain::CompactOutcome::Skipped(
            context::domain::CompactSkipReason::ResumeProtection,
        ))
    }

    async fn commit_manual_compaction(
        &self,
        _request: &context::domain::ManualCompactRequest,
    ) -> Result<context::domain::CompactOutcome, context::domain::ContextPortError> {
        Ok(context::domain::CompactOutcome::Committed(
            context::domain::CompactResult {
                summary: "manual".into(),
                recent_messages: vec![],
                source_revision: SessionRevision::new(4),
            },
        ))
    }

    async fn clear(
        &self,
        _session_id: &SessionId,
    ) -> Result<(), context::domain::ContextPortError> {
        Ok(())
    }
}

struct FakePrompt;
#[async_trait]
impl ContextPromptSource for FakePrompt {
    async fn materialize(
        &self,
        _request: &ContextRequest,
    ) -> Result<PromptMaterialization, context::ports::PromptMaterializationError> {
        Ok(PromptMaterialization {
            cacheable: vec![block("system_prompt"), block("user_guidance")],
            uncached: vec![block("current_date"), block("git_context")],
            revision: 7,
        })
    }
}

struct FakeMemory;
#[async_trait]
impl ContextMemorySource for FakeMemory {
    async fn materialize(
        &self,
        _request: &ContextRequest,
    ) -> Result<MemoryMaterialization, String> {
        Ok(MemoryMaterialization {
            blocks: vec![block("memory_context")],
            revision: 9,
        })
    }
}

fn block(kind: &str) -> SystemBlock {
    SystemBlock {
        kind: kind.into(),
        content: kind.into(),
        cacheable: true,
    }
}

fn request() -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![Message::user("pending")],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        current_date: CalendarDate::new("2026-07-15"),
        task_reminder: TaskReminderSnapshot {
            text: Some("reminder".into()),
        },
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

fn service() -> ContextApplicationService {
    ContextApplicationService::new(
        Arc::new(FakeSession),
        Arc::new(FakePrompt),
        Arc::new(FakeMemory),
    )
}

#[tokio::test]
async fn build_window_assembles_history_pending_and_fixed_extension_order() {
    let window = service().build_window(&request()).await.unwrap();
    assert_eq!(window.messages.len(), 2);
    let kinds: Vec<_> = window
        .system_blocks
        .iter()
        .map(|block| block.kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "system_prompt",
            "user_guidance",
            "memory_context",
            "active_summary",
            "cache_breakpoint",
            "current_date",
            "git_context",
            "task_reminder",
        ]
    );
}

#[tokio::test]
async fn append_delegates_finalized_step_to_session_backing() {
    let append = ContextAppend {
        session_id: SessionId::new("session"),
        expected_revision: SessionRevision::new(2),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: ContextRequestId::new("request"),
        finalize_cause: FinalizeCause::RunTerminated,
        messages: vec![Message::user("partial")],
        receipts: vec![],
        api_input_tokens: None,
        fingerprint: context::domain::ContentFingerprint::new("fp"),
    };
    let receipt = service().append_and_persist(&append).await.unwrap();
    assert_eq!(receipt.committed_revision, SessionRevision::new(3));
}

#[tokio::test]
async fn manual_compact_and_clear_session_delegate_to_session_repository() {
    use context::ports::ContextPort;

    let service = service();
    let outcome = service
        .manual_compact(&context::domain::ManualCompactRequest {
            session_id: SessionId::new("session"),
            run_id: RunId::new("run"),
            system_prompt: context::domain::SystemPromptSpec::new("system"),
            context_size: 128_000,
        })
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        context::domain::CompactOutcome::Committed(ref result)
            if result.source_revision == SessionRevision::new(4)
    ));

    service
        .clear_session(&SessionId::new("session"))
        .await
        .unwrap();
}
