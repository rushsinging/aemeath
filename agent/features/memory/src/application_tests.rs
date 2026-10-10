use super::*;
use crate::noop::NoOpMemory;
use crate::{domain::*, ports::*};
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
        coverage_end: None,
    }
}

#[tokio::test]
async fn build_prompt_reads_memory_and_owned_message_snapshot() {
    let prompt = ReflectionWorkflow::build_prompt(
        &[share::message::Message::user("remember the boundary")],
        "en",
        &NoOpMemory,
        100,
    )
    .await;

    assert!(prompt.text.contains("remember the boundary"));
    assert!(prompt.text.contains("Current project memory"));
}

/// M12：反思 prompt 不得包含失效条目。依据是 051 §8.2 的可见性矩阵与污染循环
/// 论证——基于已失效事实产出的新建议会把失效内容复制进记忆。
#[tokio::test]
async fn build_prompt_excludes_entries_that_injection_would_reject() {
    let memory = crate::adapters::InMemoryMemory::new(crate::adapters::MemoryPolicy::default())
        .expect("policy must be valid");
    let now = 1_000u64;

    let mut stale = MemoryEntry::new(
        MemoryId::now_v7(),
        now,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "the staging host is alpha",
        MemorySource::User,
    )
    .unwrap();
    stale.superseded_by = Some(MemoryId::now_v7());
    memory.write(stale).await.unwrap();

    let mut outdated = MemoryEntry::new(
        MemoryId::now_v7(),
        now,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "the billing provider was stripe",
        MemorySource::User,
    )
    .unwrap();
    outdated.outdated = true;
    memory.write(outdated).await.unwrap();

    let mut expiring = MemoryEntry::new(
        MemoryId::now_v7(),
        now,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "the incident runbook lives on paper",
        MemorySource::User,
    )
    .unwrap();
    expiring.ttl = Some(std::time::Duration::from_secs(10));
    memory.write(expiring).await.unwrap();

    let live = MemoryEntry::new(
        MemoryId::now_v7(),
        now,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "the deploy target is the staging cluster",
        MemorySource::User,
    )
    .unwrap();
    memory.write(live).await.unwrap();

    let prompt = ReflectionWorkflow::build_prompt(
        &[share::message::Message::user("keep the deployment notes")],
        "en",
        &memory,
        now + 20,
    )
    .await;

    assert!(
        prompt.text.contains("deploy target"),
        "live memory must reach the reflection: {}",
        prompt.text
    );
    for excluded in ["staging host", "billing provider", "incident runbook"] {
        assert!(
            !prompt.text.contains(excluded),
            "M12: `{excluded}` is no longer valid input, but it reached the prompt:\n{}",
            prompt.text
        );
    }
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
        &ReflectionReferenceTable::default(),
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
        &ReflectionReferenceTable::default(),
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

/// 消息摘要字符预算（#1827）：超预算时最早消息被截出 prompt（取最近部分），
/// NEVER 无界进入 prompt——PreCompact 被丢弃段与 Manual 回退全量的防爆闸。
#[tokio::test]
async fn build_prompt_truncates_messages_beyond_budget() {
    let memory = crate::noop::NoOpMemory;

    let mut messages = Vec::new();
    for index in 0..6000 {
        messages.push(share::message::Message::user(format!(
            "marker-{index:04}-padding-padding-padding"
        )));
    }

    let prompt = ReflectionWorkflow::build_prompt(&messages, "en", &memory, 1).await;
    assert!(
        !prompt.text.contains("marker-0000"),
        "超预算时最早消息必须被截出 prompt"
    );
    assert!(prompt.text.contains("marker-5999"), "最近消息必须保留");
}

/// 引用契约端到端：反思输入以行序号引用已有记忆（模型 NEVER 看到 UUID），
/// 解析链路把序号映射回真实 id；编造引用被跳过，NEVER 阻断整批。
#[tokio::test]
async fn complete_resolves_ordinal_references_end_to_end() {
    let history = RecordingHistory::default();
    let memory = crate::adapters::InMemoryMemory::new(crate::adapters::MemoryPolicy::default())
        .expect("policy must be valid");

    let first = MemoryEntry::new(
        MemoryId::now_v7(),
        1,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "old deploy target",
        MemorySource::User,
    )
    .unwrap();
    let second = MemoryEntry::new(
        MemoryId::now_v7(),
        1,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "legacy service port",
        MemorySource::User,
    )
    .unwrap();
    memory.write(first.clone()).await.unwrap();
    memory.write(second.clone()).await.unwrap();

    let prompt = ReflectionWorkflow::build_prompt(&[], "en", &memory, 100).await;
    assert!(prompt.text.contains("[M1]"), "反思输入 MUST 携带行序号");
    assert!(
        !prompt.text.contains(&first.id.to_string()),
        "UUID NEVER 进入反思 prompt"
    );

    let result = ReflectionWorkflow::complete(
        &history,
        &memory,
        &identity(),
        r#"{
            "suggested_memories": [{
                "category": "decision",
                "content": "the deploy target moved to the new cluster",
                "supersedes": ["M2"]
            }],
            "outdated_memories": ["M1", "some-tag-slug"]
        }"#,
        &prompt.references,
        "en",
        true,
        ReflectionTokenUsage::default(),
        5,
    )
    .await
    .unwrap();

    assert_eq!(result.error_category, None, "编造引用 NEVER 让整批失败");
    assert_eq!(
        result.output.outdated_memories,
        vec![first.id.to_string()],
        "序号 M1 必须映射到第一条记忆，编造引用被跳过"
    );
    let applied = result.apply_result.expect("apply outcome must be recorded");
    assert_eq!(applied.suggestions_added, 1);
    assert_eq!(applied.outdated_marked, 1);
    assert_eq!(applied.superseded, 1);

    // 取代链落在正确条目（M2 → second），而非被序号错位或编造引用波及。
    let entries = memory.list(Some(MemoryLayer::Project)).await;
    let superseded_entry = entries
        .iter()
        .find(|entry| entry.id == second.id)
        .expect("second entry remains");
    assert!(superseded_entry.superseded_by.is_some());
    let first_entry = entries
        .iter()
        .find(|entry| entry.id == first.id)
        .expect("first entry remains");
    assert!(first_entry.outdated);
}

/// 悬挂 running 收口：进程退出留下的超龄 Running 记录被收口为
/// Failed(Interrupted)；未超龄的（并发中的运行）与终态记录保持不动。
#[tokio::test]
async fn reap_stale_running_closes_only_age_exceeded_running_records() {
    let history = RecordingHistory::default();
    for (id, timestamp) in [("stale-running", 100u64), ("fresh-running", 1_500u64)] {
        ReflectionWorkflow::append_running(
            &history,
            &ReflectionExecutionIdentity {
                id: id.to_string(),
                timestamp,
                trigger: ReflectionTrigger::Interval,
                coverage_end: None,
            },
        )
        .await
        .unwrap();
    }
    history
        .upsert(&ReflectionRecord::failed(
            "old-failure".to_string(),
            100,
            ReflectionTrigger::Manual,
            ReflectionErrorCategory::TimedOut,
            5,
        ))
        .await
        .unwrap();

    let reaped = ReflectionWorkflow::reap_stale_running(&history, 1_600, 600)
        .await
        .unwrap();

    assert_eq!(reaped, 1);
    let summaries = history.list(10).await.unwrap();
    let stale = summaries
        .iter()
        .find(|record| record.id == "stale-running")
        .expect("stale record survives as a terminal fact");
    assert_eq!(stale.status, ReflectionStatus::Failed);
    assert_eq!(
        stale.error_category,
        Some(ReflectionErrorCategory::Interrupted)
    );
    let fresh = summaries
        .iter()
        .find(|record| record.id == "fresh-running")
        .expect("fresh record remains");
    assert_eq!(fresh.status, ReflectionStatus::Running);
    let failure = summaries
        .iter()
        .find(|record| record.id == "old-failure")
        .expect("terminal record remains");
    assert_eq!(
        failure.error_category,
        Some(ReflectionErrorCategory::TimedOut)
    );
}
