use super::*;
use crate::{
    MemoryError, NoOpMemory, ReflectionApplyStatus, ReflectionHistoryQuery, ReflectionSafeSummary,
};
use async_trait::async_trait;
use std::sync::Mutex;

#[derive(Default)]
struct RecordingHistory {
    records: Mutex<Vec<ReflectionRecord>>,
}

#[async_trait]
impl ReflectionHistoryQuery for RecordingHistory {
    async fn list(&self, limit: usize) -> Result<Vec<ReflectionSafeSummary>, MemoryError> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .rev()
            .take(limit)
            .map(ReflectionRecord::safe_summary)
            .collect())
    }
}

#[async_trait]
impl ReflectionHistoryStore for RecordingHistory {
    async fn append(&self, record: &ReflectionRecord) -> Result<(), MemoryError> {
        self.records.lock().unwrap().push(record.clone());
        Ok(())
    }

    async fn upsert(&self, record: &ReflectionRecord) -> Result<(), MemoryError> {
        let mut records = self.records.lock().unwrap();
        if let Some(existing) = records.iter_mut().find(|item| item.id == record.id) {
            *existing = record.clone();
        } else {
            records.push(record.clone());
        }
        Ok(())
    }
}

fn identity() -> ReflectionExecutionIdentity {
    ReflectionExecutionIdentity {
        id: "reflection-id".to_string(),
        timestamp: 42,
        trigger: ReflectionTrigger::PreCompact,
    }
}

#[test]
fn build_prompt_reads_memory_and_owned_message_snapshot() {
    let prompt = ReflectionWorkflow::build_prompt(
        &[share::message::Message::user("remember the boundary")],
        "en",
        &NoOpMemory,
    );

    assert!(prompt.contains("remember the boundary"));
    assert!(prompt.contains("Current project memory"));
}

#[tokio::test]
async fn completion_parses_applies_and_persists_memory_owned_record() {
    let history = RecordingHistory::default();
    ReflectionWorkflow::append_running(&history, &identity())
        .await
        .unwrap();

    let result = ReflectionWorkflow::complete(
        &history,
        &NoOpMemory,
        &identity(),
        r#"{"deviations":["drift"],"suggested_memories":[]}"#,
        "en",
        true,
        ReflectionTokenUsage {
            input_tokens: 11,
            output_tokens: 22,
        },
        7,
    )
    .await
    .unwrap();

    assert_eq!(result.output.deviations, ["drift"]);
    assert_eq!(result.record_id, "reflection-id");
    assert!(result.apply_result.is_some());
    let summaries = history.list(1).await.unwrap();
    assert_eq!(summaries[0].id, "reflection-id");
    assert_eq!(summaries[0].token_usage.unwrap().input_tokens, 11);
    assert_eq!(summaries[0].apply_status, ReflectionApplyStatus::Applied);
}

#[tokio::test]
async fn malformed_response_persists_safe_parse_failure_without_raw_text() {
    let history = RecordingHistory::default();
    let secret = "SECRET-raw-provider-response";
    let error = ReflectionWorkflow::complete(
        &history,
        &NoOpMemory,
        &identity(),
        secret,
        "en",
        false,
        ReflectionTokenUsage::default(),
        9,
    )
    .await
    .unwrap_err();

    assert_eq!(error, ReflectionWorkflowError::Unparseable);
    assert!(!error.to_string().contains(secret));
    let summaries = history.list(1).await.unwrap();
    assert_eq!(
        summaries[0].error_category,
        Some(ReflectionErrorCategory::Parse)
    );
    assert_eq!(summaries[0].duration_ms, 9);
}

#[tokio::test]
async fn runtime_failure_is_materialized_by_memory_history_workflow() {
    let history = RecordingHistory::default();
    ReflectionWorkflow::record_failure(
        &history,
        &identity(),
        ReflectionErrorCategory::TimedOut,
        12,
    )
    .await
    .unwrap();

    let summaries = history.list(1).await.unwrap();
    assert_eq!(
        summaries[0].error_category,
        Some(ReflectionErrorCategory::TimedOut)
    );
    assert_eq!(summaries[0].duration_ms, 12);
}
