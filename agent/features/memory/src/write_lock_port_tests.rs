//! #1981：MemoryPort 写许可互斥（进程内）。

use crate::adapters::{InMemoryMemory, MemoryPolicy};
use crate::domain::*;
use crate::ports::{MemoryPort, WriteResult};
use std::sync::Arc;
use std::time::Duration;

fn fact_entry(content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        100,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_permit_blocks_concurrent_update_until_released() {
    let memory = Arc::new(InMemoryMemory::new(MemoryPolicy::default()).unwrap());
    let entry = fact_entry("seed");
    let id = entry.id.clone();
    let written = memory.write(entry).await.expect("seed write");
    assert!(
        matches!(written, WriteResult::Added { .. }),
        "seed must be Added, got {written:?}"
    );

    let permit = memory.acquire_write_permit().await.expect("permit");
    let blocked_memory = Arc::clone(&memory);
    let blocked_id = id.clone();
    let blocked = tokio::spawn(async move {
        blocked_memory
            .update(&blocked_id, "changed-under-lock")
            .await
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !blocked.is_finished(),
        "持有 write permit 时 update MUST 阻塞"
    );

    drop(permit);
    let updated = tokio::time::timeout(Duration::from_secs(2), blocked)
        .await
        .expect("释放后 update 应完成")
        .expect("join")
        .expect("update ok");
    assert!(updated);

    let listed = memory.list(Some(MemoryLayer::Project)).await;
    let content = listed
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.content.as_str())
        .expect("entry present");
    assert_eq!(content, "changed-under-lock");
}

#[tokio::test]
async fn apply_reflection_under_outer_permit_does_not_reacquire() {
    let memory = InMemoryMemory::new(MemoryPolicy {
        max_entries: 10,
        similarity_threshold: 0.9,
        target_active_entries: 8,
    })
    .unwrap();
    let permit = memory.acquire_write_permit().await.unwrap();
    let output = ReflectionOutput {
        deviations: vec![],
        suggested_memories: vec![],
        outdated_memories: vec![],
    };
    // 外层已持锁时走 under_permit，不得二次取锁死锁。
    memory
        .apply_reflection_under_permit(&output, &permit)
        .await
        .expect("under_permit apply");
}
