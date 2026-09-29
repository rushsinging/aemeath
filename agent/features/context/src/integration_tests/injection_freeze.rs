//! Contract for the frozen injection window (#1777).
//!
//! The memory block is part of the cacheable system prefix: re-retrieving it
//! every turn changes bytes the provider could otherwise reuse. The block is
//! therefore materialized once per session and reused, with exactly one refresh
//! point — a committed compaction, which rewrites the conversation the memory
//! was retrieved for.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::{
    ContextApplicationService, ContextMemorySource, ContextPort, ContextRequestData,
    ContextRequestId, MemoryMaterialization, SessionId, SystemBlock, SystemPromptSpecData,
};
use async_trait::async_trait;
use sdk::{RunId, RunStepId};

use super::application_service_contract::{request, simple_fake_session};

/// Counts how often the memory source was actually consulted.
struct CountingMemory {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ContextMemorySource for CountingMemory {
    async fn materialize(
        &self,
        _request: &ContextRequestData,
    ) -> Result<MemoryMaterialization, String> {
        let call = self.calls.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(MemoryMaterialization {
            blocks: vec![SystemBlock {
                kind: "memory_context".into(),
                content: format!("memory generation {call}"),
                cacheable: true,
                cache_break: false,
            }],
            revision: call as u64,
        })
    }
}

fn service_with_memory(memory: Arc<CountingMemory>) -> ContextApplicationService {
    ContextApplicationService::new(
        Arc::new(simple_fake_session()),
        Arc::new(crate::adapters::BaselinePromptSource),
        memory,
    )
}

fn memory_content(window: &crate::ContextWindowData) -> String {
    window
        .system_blocks
        .iter()
        .find(|block| block.kind == "memory_context")
        .map(|block| block.content.clone())
        .expect("the memory block is always present in this fixture")
}

/// The first window of a session carries the memory; later turns reuse it.
#[tokio::test]
async fn the_injection_is_materialized_once_per_session_and_then_frozen() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service_with_memory(Arc::new(CountingMemory {
        calls: Arc::clone(&calls),
    }));

    let first = service.build_window(&request()).await.unwrap();
    let second = service.build_window(&request()).await.unwrap();
    let third = service.build_window(&request()).await.unwrap();

    assert_eq!(calls.load(Ordering::Relaxed), 1, "retrieved exactly once");
    assert_eq!(memory_content(&first), "memory generation 1");
    assert_eq!(
        memory_content(&second),
        "memory generation 1",
        "a frozen prefix must keep identical bytes"
    );
    assert_eq!(memory_content(&third), "memory generation 1");
}

/// A new session starts its own first injection even on the same port.
#[tokio::test]
async fn a_new_session_injects_again() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service_with_memory(Arc::new(CountingMemory {
        calls: Arc::clone(&calls),
    }));

    service.build_window(&request()).await.unwrap();
    let other_session = ContextRequestData {
        session_id: SessionId::new("other-session"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![],
        invocation_reminders: vec![],
        system_prompt: SystemPromptSpecData::new("system"),
        model_id: "fake/model".into(),
        effective_reasoning: share::reasoning::ReasoningLevel::Off,
        language: crate::Language::new("en"),
        agent_roles: std::collections::HashMap::new(),
        config_snapshot: share::config::domain::snapshot::ConfigSnapshot::new(
            share::config::Config::default(),
        ),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        heuristic_calibration: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    };

    let window = service.build_window(&other_session).await.unwrap();

    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "a new session injects again"
    );
    assert_eq!(memory_content(&window), "memory generation 2");
}

/// A committed compaction is the one refresh point: the conversation it
/// rewrote is exactly what the memory was retrieved for.
#[tokio::test]
async fn a_committed_compaction_refreshes_the_frozen_injection() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service_with_memory(Arc::new(CountingMemory {
        calls: Arc::clone(&calls),
    }));

    let before = service.build_window(&request()).await.unwrap();
    service
        .manual_compact(&crate::ManualCompactRequestData {
            session_id: SessionId::new("session"),
            run_id: RunId::new("run"),
            system_prompt: SystemPromptSpecData::new("system"),
            context_size: 128_000,
            progress: None,
            task_snapshot: None,
        })
        .await
        .unwrap();
    let after = service.build_window(&request()).await.unwrap();

    assert_eq!(memory_content(&before), "memory generation 1");
    assert_eq!(
        memory_content(&after),
        "memory generation 2",
        "a committed compaction must re-retrieve memory"
    );
}

/// A compaction that did not commit changed nothing, so the frozen block stays.
#[tokio::test]
async fn a_skipped_compaction_keeps_the_frozen_injection() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service_with_memory(Arc::new(CountingMemory {
        calls: Arc::clone(&calls),
    }));

    let before = service.build_window(&request()).await.unwrap();
    // The fixture's automatic compaction reports `Skipped`.
    service
        .compact(&crate::CompactRequestData {
            run_id: RunId::new("run"),
            source_revision: crate::SessionRevision::new(3),
            source: request(),
            trigger: crate::CompactTrigger::Automatic,
            progress: None,
            task_snapshot: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
        })
        .await
        .unwrap();
    let after = service.build_window(&request()).await.unwrap();

    assert_eq!(memory_content(&before), "memory generation 1");
    assert_eq!(
        memory_content(&after),
        "memory generation 1",
        "a skipped compaction must not disturb the frozen prefix"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

/// Clearing a session drops the frozen block: the next window is a first
/// injection for whatever starts next.
#[tokio::test]
async fn clearing_a_session_releases_the_frozen_injection() {
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service_with_memory(Arc::new(CountingMemory {
        calls: Arc::clone(&calls),
    }));

    service.build_window(&request()).await.unwrap();
    service
        .clear_session(&SessionId::new("session"))
        .await
        .unwrap();
    service.build_window(&request()).await.unwrap();

    assert_eq!(
        calls.load(Ordering::Relaxed),
        2,
        "clear starts a fresh epoch"
    );
}
