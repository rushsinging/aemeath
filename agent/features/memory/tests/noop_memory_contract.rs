use memory::*;

#[tokio::test]
async fn noop_memory_is_explicitly_disabled_and_has_no_mutation_effects() {
    let port = NoOpMemory;
    let inject = port.retrieve_for_inject(&MemoryQuery {
        limit: 10,
        layer: None,
        category: None,
        now: 100,
    });
    assert_eq!(inject.mode, MemoryRetrievalMode::Disabled);
    assert!(inject.hits.is_empty());

    let search = port.search(&MemorySearchQuery {
        text: "anything".into(),
        limit: 10,
        layer: None,
        category: None,
        include_archive: true,
        now: 100,
    });
    assert_eq!(search.mode, MemoryRetrievalMode::Disabled);
    assert!(search.hits.is_empty());

    let entry = MemoryEntry::new(
        MemoryId::now_v7(),
        100,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        "must not persist",
        MemorySource::User,
    )
    .unwrap();
    let id = entry.id;
    assert_eq!(port.write(entry).await.unwrap(), WriteResult::NoOp);
    assert!(!port.update(&id, "new").await.unwrap());
    assert!(!port.delete(&id).await.unwrap());
    assert!(!port.pin(&id, true).await.unwrap());
    assert!(!port.mark_outdated(&id).await.unwrap());
    assert_eq!(
        port.apply_reflection(&ReflectionOutput).await.unwrap(),
        ReflectionApplyResult::default()
    );
    port.archive(&[id]).await.unwrap();
    assert_eq!(
        port.compact().await.unwrap(),
        CompactResult {
            archived: 0,
            remaining: 0,
        }
    );
    assert!(port.list(None).is_empty());
    assert_eq!(port.stats(), MemoryStats::default());
}
