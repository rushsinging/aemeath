use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use provider::ReasoningLevel;
use sdk::RunId;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::{ContentBlock, Message};

use super::performance::{capture, percentiles_ns};
use super::service::ContextApplicationService;
use crate::domain::{
    ContextAppend, ContextRequest, ContextRequestId, Language, RunStepId, SessionId,
    SessionRevision, SystemBlock, SystemPromptSpec, TaskReminderSnapshot,
};
use crate::ports::{
    ContextMemorySource, ContextPort, ContextPromptSource, MemoryMaterialization,
    PromptMaterialization, SessionRepository, SessionSnapshot,
};

struct BaselineSession {
    revision: SessionRevision,
    messages: Vec<Message>,
}

#[async_trait]
impl SessionRepository for BaselineSession {
    async fn snapshot(&self, _session_id: &SessionId) -> Result<SessionSnapshot, String> {
        Ok(SessionSnapshot {
            revision: self.revision,
            messages: self.messages.clone(),
            active_summary: Some("summary".into()),
        })
    }

    async fn append_finalized(
        &self,
        append: &ContextAppend,
    ) -> Result<crate::domain::AppendReceipt, crate::domain::ContextAppendError> {
        Ok(crate::domain::AppendReceipt {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: self.revision,
            fingerprint: append.fingerprint.clone(),
        })
    }

    async fn commit_compaction(
        &self,
        _request: &crate::domain::CompactRequest,
    ) -> Result<crate::domain::CompactOutcome, crate::domain::ContextPortError> {
        Ok(crate::domain::CompactOutcome::Skipped(
            crate::domain::CompactSkipReason::ResumeProtection,
        ))
    }

    async fn commit_manual_compaction(
        &self,
        _request: &crate::domain::ManualCompactRequest,
    ) -> Result<crate::domain::CompactOutcome, crate::domain::ContextPortError> {
        Ok(crate::domain::CompactOutcome::Skipped(
            crate::domain::CompactSkipReason::ResumeProtection,
        ))
    }

    async fn clear(&self, _session_id: &SessionId) -> Result<(), crate::domain::ContextPortError> {
        Ok(())
    }
}

struct BaselinePrompt;

#[async_trait]
impl ContextPromptSource for BaselinePrompt {
    async fn materialize(
        &self,
        _request: &ContextRequest,
    ) -> Result<PromptMaterialization, crate::domain::PromptMaterializationError> {
        Ok(PromptMaterialization {
            cacheable: vec![block("system_prompt"), block("user_guidance")],
            uncached: vec![block("runtime_context")],
            revision: 7,
        })
    }
}

struct BaselineMemory;

#[async_trait]
impl ContextMemorySource for BaselineMemory {
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
        content: kind.repeat(8),
        cacheable: true,
        cache_break: false,
    }
}

fn request(last_api_total_tokens: Option<u64>) -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("baseline-session"),
        request_id: ContextRequestId::new("baseline-request"),
        run_id: RunId::new("baseline-run"),
        step_id: RunStepId::new("baseline-step"),
        pending_messages: vec![Message::user("pending")],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        task_reminder: TaskReminderSnapshot::default(),
        language: Language::new("zh"),
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

fn service(messages: Vec<Message>, revision: u64) -> ContextApplicationService {
    ContextApplicationService::new(
        Arc::new(BaselineSession {
            revision: SessionRevision::new(revision),
            messages,
        }),
        Arc::new(BaselinePrompt),
        Arc::new(BaselineMemory),
    )
}

fn tool_result_message(bytes: usize) -> (Message, usize) {
    let content = serde_json::Value::String("x".repeat(bytes));
    let serialized_bytes = content.to_string().len();
    (
        Message {
            role: share::message::Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool-1".into(),
                content,
                is_error: false,
                text: None,
            }],
            metadata: None,
        },
        serialized_bytes,
    )
}

#[tokio::test]
async fn build_window_capture_reports_structure_phases_and_actual_usage() {
    let (tool_result, expected_bytes) = tool_result_message(1_024);
    let context = service(vec![Message::user("history"), tool_result], 42);

    let (window, metrics) = capture(context.build_window(&request(Some(777)))).await;
    let window = window.unwrap();

    assert_eq!(window.backing_revision, SessionRevision::new(42));
    assert_eq!(metrics.build_calls, 1);
    assert_eq!(metrics.snapshot_calls, 1);
    assert_eq!(metrics.prompt_calls, 1);
    assert_eq!(metrics.memory_calls, 1);
    assert_eq!(metrics.assembly_calls, 1);
    assert_eq!(metrics.decision_calls, 1);
    assert_eq!(metrics.backing_revision, 42);
    assert_eq!(metrics.snapshot_messages, 2);
    assert_eq!(metrics.pending_messages, 1);
    assert_eq!(metrics.final_messages, 3);
    assert_eq!(metrics.system_blocks, 5);
    assert_eq!(metrics.tool_result_blocks, 1);
    assert_eq!(metrics.tool_result_content_bytes, expected_bytes as u64);
    assert_eq!(metrics.provider_actual_tokens, Some(777));
    assert_eq!(metrics.decision_token_count, 777);
    assert_eq!(
        metrics.decision_reason,
        Some(crate::domain::DecisionReason::ActualProviderUsage)
    );
    assert!(metrics.estimated_total_tokens > 0);
}

#[tokio::test]
async fn repeated_build_captures_do_not_accumulate_structure_counts() {
    let (tool_result, _) = tool_result_message(4_096);
    let context = service(vec![Message::user("history"), tool_result], 9);
    let request = request(None);

    let (_, first) = capture(context.build_window(&request)).await;
    let (_, second) = capture(context.build_window(&request)).await;

    assert_eq!(first.backing_revision, second.backing_revision);
    assert_eq!(first.snapshot_messages, second.snapshot_messages);
    assert_eq!(first.final_messages, second.final_messages);
    assert_eq!(first.system_blocks, second.system_blocks);
    assert_eq!(first.tool_result_blocks, second.tool_result_blocks);
    assert_eq!(
        first.tool_result_content_bytes,
        second.tool_result_content_bytes
    );
    assert_eq!(first.estimated_total_tokens, second.estimated_total_tokens);
    assert_eq!(first.decision_token_count, second.decision_token_count);
    assert_eq!(first.decision_reason, second.decision_reason);
    assert_eq!(second.build_calls, 1);
}

fn workload_messages(message_count: usize, tool_result_every: Option<usize>) -> Vec<Message> {
    (0..message_count)
        .map(|index| {
            if tool_result_every.is_some_and(|interval| index % interval == 0) {
                tool_result_message(64 * 1_024).0
            } else {
                Message::user(format!(
                    "message {index}: Context 基线 **Markdown** 与 Unicode ✓"
                ))
            }
        })
        .collect()
}

#[tokio::test]
#[ignore = "性能基线；手动运行：cargo test -p context --release context_build_release_workload -- --ignored --nocapture"]
#[allow(clippy::print_stdout)]
async fn context_build_release_workload() {
    const SAMPLES: usize = 20;
    let scenarios = [
        ("messages-100", 100, None),
        ("messages-500", 500, None),
        ("messages-1000", 1_000, None),
        ("tool-results-64k", 100, Some(10)),
    ];

    println!("\n=== #1418 Context build 性能基线（samples={SAMPLES}）===");
    for (name, message_count, tool_result_every) in scenarios {
        let context = service(
            workload_messages(message_count, tool_result_every),
            message_count as u64,
        );
        let request = request(None);
        let mut wall_samples = Vec::with_capacity(SAMPLES);
        let mut build_samples = Vec::with_capacity(SAMPLES);
        let mut decision_samples = Vec::with_capacity(SAMPLES);
        let mut representative = None;

        for _ in 0..SAMPLES {
            let started = Instant::now();
            let (window, metrics) = capture(context.build_window(&request)).await;
            window.unwrap();
            wall_samples.push(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
            build_samples.push(metrics.build_ns);
            decision_samples.push(metrics.decision_ns);
            representative = Some(metrics);
        }

        let metrics = representative.expect("workload 至少有一个样本");
        let (wall_p50, wall_p95) = percentiles_ns(&wall_samples).unwrap();
        let (build_p50, build_p95) = percentiles_ns(&build_samples).unwrap();
        let (decision_p50, decision_p95) = percentiles_ns(&decision_samples).unwrap();
        assert_eq!(metrics.final_messages, message_count + 1);
        assert_eq!(metrics.build_calls, 1);
        assert!(metrics.estimated_total_tokens > 0);
        println!(
            "scenario={name} messages={} tool_results={} tool_result_bytes={} estimated_tokens={} | wall_p50/p95={:.3}/{:.3}ms build_p50/p95={:.3}/{:.3}ms decision_p50/p95={:.3}/{:.3}ms",
            metrics.final_messages,
            metrics.tool_result_blocks,
            metrics.tool_result_content_bytes,
            metrics.estimated_total_tokens,
            wall_p50 as f64 / 1_000_000.0,
            wall_p95 as f64 / 1_000_000.0,
            build_p50 as f64 / 1_000_000.0,
            build_p95 as f64 / 1_000_000.0,
            decision_p50 as f64 / 1_000_000.0,
            decision_p95 as f64 / 1_000_000.0,
        );
    }
}
