use std::sync::Arc;

use async_trait::async_trait;
use context::adapters::CommittedMemoryRetrieveAdapter;
use context::application::ContextApplicationService;
use context::domain::session::{
    CommittedRunSlice, CommittedRunStep, CommittedStepMessages, FinalizedOutcomeProjection,
    SessionHistory,
};
use context::domain::{
    CleanupConfirmation, ContextAppend, ContextMessages, ContextRequest, ContextRequestId,
    FinalizeCause, InvocationReminder, Language, RunStepId, SessionId, SessionRevision,
    SystemBlock, SystemPromptSpec, ToolCallIdentity, ToolCallReceipt, ToolOutcomeKind,
    ToolTerminalReceipt,
};
use context::ports::{
    ContextMemorySource, ContextPort, ContextPromptSource, MemoryMaterialization,
    PromptMaterialization, SessionRepository, SessionSnapshot,
};
use memory::api::{MemoryPort, NoOpMemory};
use provider::ReasoningLevel;
use sdk::RunId;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::{ContentBlock, Message};

const CHECKPOINT: &str = "## Immutable Constraints\n- review only\n\n## Current Objective\n- inspect resume\n\n## Committed Facts\n- persisted\n\n## Uncommitted Working Set\n- none\n\n## Open Decisions / Risks\n- dynamic state\n\n## Resume Cursor\n- Next action: revalidate once\n\n## Required Revalidation\n- revalidate git\n\n## Archived Milestones\n- baseline\n\n## Continuation Status\nContinue";

struct FakeSession {
    messages: ContextMessages,
    structured_history: Option<SessionHistory>,
}

fn bounded_tool_result_message() -> Message {
    let preview = "<persisted-output>bounded preview</persisted-output>";
    Message {
        role: share::message::Role::User,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "tool".to_string(),
            content: serde_json::json!({
                "text": preview,
                "truncated": true,
                "original_chars": 50_001,
                "original_bytes": 50_001,
                "omitted_chars": 47_501,
                "blob": {
                    "status": "persisted",
                    "locator": "tool-result://session/tool"
                }
            }),
            is_error: false,
            text: Some(preview.to_string()),
        }],
        metadata: None,
    }
}

fn simple_fake_session() -> FakeSession {
    FakeSession {
        messages: vec![Message::user("history"), bounded_tool_result_message()].into(),
        structured_history: None,
    }
}

fn structured_fake_session() -> FakeSession {
    let tool_step =
        |run_id: &str, step_id: &str, call_id: &str, tool_name: &str, path: &str, text: &str| {
            let input = serde_json::json!({"file_path": path});
            let normalized_run_id = RunId::new(run_id);
            let normalized_step_id = RunStepId::new(step_id);
            CommittedRunStep {
                step_id: normalized_step_id.as_str().to_string(),
                accepted_input: None,
                outcome: Some(FinalizedOutcomeProjection {
                    finalize_cause: FinalizeCause::Completed,
                    duration_ms: None,
                    messages: CommittedStepMessages::from(vec![Message {
                        role: share::message::Role::Assistant,
                        content: vec![
                            ContentBlock::ToolUse {
                                id: call_id.into(),
                                name: tool_name.into(),
                                input: input.clone(),
                            },
                            ContentBlock::ToolResult {
                                tool_use_id: call_id.into(),
                                content: serde_json::json!({"typed": text}),
                                is_error: false,
                                text: Some(text.into()),
                            },
                        ],
                        metadata: None,
                    }]),
                    receipts: vec![],
                    api_input_tokens: None,
                    fingerprint: format!("fp-{step_id}"),
                    committed_revision: 2,
                }),
                tool_receipts: vec![ToolCallReceipt {
                    identity: ToolCallIdentity {
                        session_id: SessionId::new("session"),
                        run_id: normalized_run_id,
                        step_id: normalized_step_id,
                        runtime_call_id: call_id.into(),
                        provider_call_id: Some(call_id.into()),
                        tool_name: tool_name.into(),
                        call_index: 0,
                        agent: false,
                    },
                    input_preview: input.to_string(),
                    state: context::domain::ToolCallState::Terminal(ToolTerminalReceipt::new(
                        ToolOutcomeKind::Success,
                        "terminal",
                        CleanupConfirmation::NotApplicable,
                    )),
                }],
            }
        };
    let structured_history = SessionHistory::from_slices(vec![
        CommittedRunSlice::new(
            RunId::new("old-read-run").as_ref(),
            vec![tool_step(
                "old-read-run",
                "old-read-step",
                "old-read-call",
                "Read",
                "/repo/src/lib.rs",
                "obsolete read body",
            )],
        ),
        CommittedRunSlice::new(
            RunId::new("old-search-run").as_ref(),
            vec![tool_step(
                "old-search-run",
                "old-search-step",
                "old-search-call",
                "WebSearch",
                "/search",
                "old search body",
            )],
        ),
        CommittedRunSlice::new(
            RunId::new("write-run").as_ref(),
            vec![tool_step(
                "write-run",
                "write-step",
                "write-call",
                "Write",
                "/repo/src/lib.rs",
                "written",
            )],
        ),
        CommittedRunSlice::new(
            RunId::new("recent-1").as_ref(),
            vec![tool_step(
                "recent-1",
                "recent-1-step",
                "recent-1-call",
                "Read",
                "/repo/recent-1.rs",
                "recent one",
            )],
        ),
        CommittedRunSlice::new(
            RunId::new("recent-2").as_ref(),
            vec![tool_step(
                "recent-2",
                "recent-2-step",
                "recent-2-call",
                "Read",
                "/repo/recent-2.rs",
                "recent two",
            )],
        ),
    ]);
    let messages = ContextMessages::from_committed_steps(
        structured_history
            .iter()
            .flat_map(|run| run.steps.iter())
            .flat_map(|step| step.outcome.iter().map(|outcome| outcome.messages.as_arc()))
            .collect(),
        Vec::new(),
    );
    FakeSession {
        messages,
        structured_history: Some(structured_history),
    }
}

#[async_trait]
impl SessionRepository for FakeSession {
    async fn snapshot(&self, _session_id: &SessionId) -> Result<SessionSnapshot, String> {
        Ok(SessionSnapshot {
            revision: SessionRevision::new(2),
            messages: self.messages.clone(),
            structured_history: self.structured_history.clone(),
            active_summary: Some(CHECKPOINT.into()),
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
            uncached: Vec::new(),
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
        cache_break: false,
    }
}

fn request() -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![Message::user("pending")],
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
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

fn service_with_session(session: FakeSession) -> ContextApplicationService {
    ContextApplicationService::new(
        Arc::new(session),
        Arc::new(FakePrompt),
        Arc::new(FakeMemory),
    )
}

fn service() -> ContextApplicationService {
    service_with_session(simple_fake_session())
}

fn request_with_context_reduction(
    snip_enabled: bool,
    microcompact_enabled: bool,
) -> ContextRequest {
    let mut request = request();
    let mut config = Config::default();
    config.context.snip_enabled = snip_enabled;
    config.context.microcompact_enabled = microcompact_enabled;
    request.config_snapshot = ConfigSnapshot::new(config);
    request
}

fn result_text(window: &context::domain::ContextWindow, call_id: &str) -> String {
    window
        .messages
        .iter()
        .flat_map(|message| message.content.iter())
        .find_map(|block| match block {
            ContentBlock::ToolResult {
                tool_use_id, text, ..
            } if tool_use_id == call_id => text.clone(),
            _ => None,
        })
        .unwrap()
}

#[tokio::test]
async fn build_window_honors_independent_context_reduction_switches() {
    let cases = [
        (false, false, "obsolete read body", "old search body"),
        (
            true,
            false,
            "[Superseded tool result: Read /repo/src/lib.rs]",
            "old search body",
        ),
        (
            false,
            true,
            "[Microcompacted tool result: Read]",
            "[Microcompacted tool result: WebSearch]",
        ),
        (
            true,
            true,
            "[Superseded tool result: Read /repo/src/lib.rs]",
            "[Microcompacted tool result: WebSearch]",
        ),
    ];

    for (snip_enabled, microcompact_enabled, expected_read, expected_search) in cases {
        let mut request = request_with_context_reduction(snip_enabled, microcompact_enabled);
        request.run_id = RunId::new("active-run");
        let window = service_with_session(structured_fake_session())
            .build_window(&request)
            .await
            .unwrap();

        assert_eq!(result_text(&window, "old-read-call"), expected_read);
        assert_eq!(result_text(&window, "old-search-call"), expected_search);
    }
}

#[tokio::test]
async fn build_window_applies_l2_then_l3_before_prompt_memory_and_token_decision() {
    let mut request = request();
    request.run_id = RunId::new("active-run");
    let session = structured_fake_session();
    let canonical_bytes_before = serde_json::to_vec(
        session
            .structured_history
            .as_ref()
            .expect("structured fixture"),
    )
    .unwrap();
    let window = service_with_session(session)
        .build_window(&request)
        .await
        .unwrap();
    let repeated_session = structured_fake_session();
    let canonical_bytes_after = serde_json::to_vec(
        repeated_session
            .structured_history
            .as_ref()
            .expect("structured fixture"),
    )
    .unwrap();
    let repeated_window = service_with_session(repeated_session)
        .build_window(&request)
        .await
        .unwrap();

    assert_eq!(canonical_bytes_before, canonical_bytes_after);
    assert_eq!(
        serde_json::to_vec(&window.messages).unwrap(),
        serde_json::to_vec(&repeated_window.messages).unwrap()
    );
    let result_text = |call_id: &str| {
        window
            .messages
            .iter()
            .flat_map(|message| message.content.iter())
            .find_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id, text, ..
                } if tool_use_id == call_id => text.clone(),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        result_text("old-read-call"),
        "[Superseded tool result: Read /repo/src/lib.rs]"
    );
    assert_eq!(
        result_text("old-search-call"),
        "[Microcompacted tool result: WebSearch]"
    );
    assert_eq!(result_text("recent-1-call"), "recent one");
    assert_eq!(result_text("recent-2-call"), "recent two");
    assert!(!window
        .messages
        .iter()
        .any(|message| message.text_content().contains("obsolete read body")));
    assert!(!window
        .messages
        .iter()
        .any(|message| message.text_content().contains("old search body")));
    assert!(window.token_estimation.message_tokens > 0);
}

#[tokio::test]
async fn repeated_windows_preserve_bounded_tool_result_bytes_across_steps() {
    let service = service();
    let first = service.build_window(&request()).await.unwrap();
    let mut next_request = request();
    next_request.request_id = ContextRequestId::new("request-next");
    next_request.step_id = RunStepId::new("step-next");
    next_request.pending_messages = vec![Message::user("next pending")];
    let second = service.build_window(&next_request).await.unwrap();

    let first_tool_result = serde_json::to_vec(&first.messages[1]).unwrap();
    let second_tool_result = serde_json::to_vec(&second.messages[1]).unwrap();
    assert_eq!(first_tool_result, second_tool_result);
    assert_eq!(
        serde_json::to_vec(&first.messages[1].to_llm_view()).unwrap(),
        serde_json::to_vec(&second.messages[1].to_llm_view()).unwrap()
    );
    assert!(!String::from_utf8(first_tool_result)
        .unwrap()
        .contains("FULL_PAYLOAD_SENTINEL"));
}

#[tokio::test]
async fn committed_memory_adapter_switches_from_noop_to_active_memory_for_context() {
    let memory: Arc<std::sync::RwLock<Arc<dyn MemoryPort>>> =
        Arc::new(std::sync::RwLock::new(Arc::new(NoOpMemory)));
    let source = CommittedMemoryRetrieveAdapter::new(Arc::clone(&memory));

    let before = source.materialize(&request()).await.unwrap();
    assert!(before.blocks.is_empty());

    let active = memory::InMemoryMemory::new(memory::MemoryPolicy::default()).unwrap();
    let entry = memory::MemoryEntry::new(
        memory::MemoryId::now_v7(),
        100,
        memory::MemoryLayer::Project,
        memory::MemoryCategory::Fact,
        "active memory fact",
        memory::MemorySource::User,
    )
    .unwrap();
    active.write(entry).await.unwrap();
    *memory.write().unwrap() = Arc::new(active);

    let after = source.materialize(&request()).await.unwrap();
    assert_eq!(after.blocks.len(), 1);
    assert_eq!(after.blocks[0].kind, "memory_context");
    assert!(after.blocks[0].cacheable);
    assert!(after.blocks[0].content.contains("active memory fact"));
}

#[tokio::test]
async fn build_window_keeps_committed_history_shared_and_pending_owned() {
    let window = service().build_window(&request()).await.unwrap();

    assert_eq!(window.messages.len(), 3);
    assert_eq!(window.messages[0].text_content(), "history");
    assert!(window.messages[1].has_tool_results());
    assert_eq!(window.messages[2].text_content(), "pending");
}

#[tokio::test]
async fn build_window_assembles_history_pending_and_fixed_extension_order() {
    let window = service().build_window(&request()).await.unwrap();
    assert_eq!(window.messages.len(), 3);
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
        ]
    );
    assert!(window
        .system_blocks
        .iter()
        .all(|block| block.kind != "task_reminder"));
    let cache_breaks: Vec<_> = window
        .system_blocks
        .iter()
        .filter(|block| block.cache_break)
        .map(|block| block.kind.as_str())
        .collect();
    assert_eq!(cache_breaks, vec!["active_summary"]);
    let summaries = window
        .system_blocks
        .iter()
        .filter(|block| block.kind == "active_summary")
        .collect::<Vec<_>>();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].content, CHECKPOINT);
    assert!(summaries[0].cacheable);
    assert!(matches!(
        &window.messages[2].content[0],
        ContentBlock::Text { text } if text == "pending"
    ));
    assert!(window.token_estimation.message_tokens > 0);
}

#[tokio::test]
async fn build_window_omits_date_and_dynamic_system_context() {
    let window = service().build_window(&request()).await.unwrap();

    assert!(window.system_blocks.iter().all(|block| !matches!(
        block.kind.as_str(),
        "current_date" | "dynamic_system_context"
    )));
}

#[tokio::test]
async fn build_window_renders_invocation_reminders_once_in_stable_order() {
    let mut request = request();
    request.invocation_reminders = vec![
        InvocationReminder::model_guidance_mismatch("session/model", "run/model"),
        InvocationReminder::guidance_sources_changed(),
        InvocationReminder::task_progress(context::domain::TaskProgressReminder {
            total: 2,
            completed: 0,
            items: vec![
                context::domain::TaskProgressReminderItem {
                    sequence: 1,
                    subject: "task one".into(),
                    status: context::domain::TaskProgressStatus::InProgress,
                    blocked_by_sequences: vec![],
                },
                context::domain::TaskProgressReminderItem {
                    sequence: 2,
                    subject: "task two".into(),
                    status: context::domain::TaskProgressStatus::Pending,
                    blocked_by_sequences: vec![1],
                },
            ],
            hidden_count: 0,
        }),
    ];

    let window = service().build_window(&request).await.unwrap();
    let rendered: Vec<_> = window
        .messages
        .iter()
        .map(Message::text_content)
        .filter(|text| text.contains("<system-reminder>"))
        .collect();

    assert_eq!(rendered.len(), 3);
    assert!(rendered[0].contains("当前任务进度"));
    assert!(rendered[1].contains("guidance 来源已变更"));
    assert!(rendered[2].contains("Session 冻结模型 session/model"));
    assert!(window.token_estimation.message_tokens > 0);
    assert_eq!(
        window
            .system_blocks
            .iter()
            .filter(|block| block.cache_break)
            .count(),
        1
    );
}

#[tokio::test]
async fn build_window_without_invocation_reminders_keeps_messages_unchanged() {
    let window = service().build_window(&request()).await.unwrap();

    assert_eq!(window.messages.len(), 3);
    assert!(window
        .messages
        .iter()
        .all(|message| !message.text_content().contains("<system-reminder>")));
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
        duration_ms: None,
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
    let service = service();
    let outcome = service
        .manual_compact(&context::domain::ManualCompactRequest {
            session_id: SessionId::new("session"),
            run_id: RunId::new("run"),
            system_prompt: context::domain::SystemPromptSpec::new("system"),
            context_size: 128_000,
            progress: None,
            task_context: None,
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
