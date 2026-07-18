use memory::*;
use std::time::Duration;

fn entry(_id: &str, content: &str, now: u64) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        now,
        MemoryLayer::Project,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .unwrap()
}

#[tokio::test]
async fn in_memory_fake_satisfies_memory_port_contract() {
    let port = InMemoryMemory::new(MemoryPolicy {
        max_entries: 2,
        similarity_threshold: 0.8,
    })
    .unwrap();

    let first = entry("first", "Rust memory port", 100);
    assert_eq!(
        port.write(first.clone()).await.unwrap(),
        WriteResult::Added { id: first.id }
    );
    assert_eq!(port.revision(), 1);

    let duplicate = entry("duplicate", "rust memory port", 101);
    assert_eq!(
        port.write(duplicate).await.unwrap(),
        WriteResult::Merged {
            existing_id: first.id
        }
    );

    let mut pinned = entry("pinned", "stable architecture", 102);
    pinned.pinned = true;
    port.write(pinned.clone()).await.unwrap();
    let full = port
        .write(entry("third", "unrelated content", 103))
        .await
        .unwrap();
    assert!(
        matches!(full, WriteResult::NeedsEviction { candidates } if candidates.iter().all(|entry| !entry.pinned))
    );

    let before = port.revision();
    let injection = port.retrieve_for_inject(&MemoryQuery {
        limit: 10,
        layer: None,
        category: None,
        now: 200,
    });
    assert_eq!(injection.mode, MemoryRetrievalMode::InjectionPriority);
    assert!(injection.hits.iter().all(|hit| {
        hit.location == MemoryLocation::Active
            && !hit.outdated
            && !hit.ttl_expired
            && hit.relevance.is_none()
    }));
    assert_eq!(port.revision(), before);

    port.archive(std::slice::from_ref(&first.id)).await.unwrap();
    let search = port.search(&MemorySearchQuery {
        text: "memory".to_string(),
        limit: 10,
        layer: None,
        category: None,
        include_archive: true,
        now: 200,
    });
    assert_eq!(search.mode, MemoryRetrievalMode::ExplicitSearch);
    assert!(search.hits.iter().any(|hit| {
        hit.entry.id == first.id
            && hit.location == MemoryLocation::Archive
            && hit.relevance.is_some()
    }));
}

#[tokio::test]
async fn explicit_search_returns_ineligible_archive_without_panicking() {
    let port = InMemoryMemory::new(MemoryPolicy::default()).unwrap();
    let mut old = entry("old", "searchable legacy", 100);
    old.outdated = true;
    old.ttl = Some(Duration::from_secs(1));
    port.write(old.clone()).await.unwrap();
    port.archive(std::slice::from_ref(&old.id)).await.unwrap();

    let before = port.revision();
    let result = port.search(&MemorySearchQuery {
        text: "searchable".to_string(),
        limit: 10,
        layer: None,
        category: None,
        include_archive: true,
        now: 102,
    });
    let hit = &result.hits[0];
    assert!(hit.outdated);
    assert!(hit.ttl_expired);
    assert_eq!(hit.location, MemoryLocation::Archive);
    assert_eq!(port.revision(), before);
}

#[tokio::test]
async fn mutations_are_typed_and_queries_do_not_change_revision() {
    let port = InMemoryMemory::new(MemoryPolicy::default()).unwrap();
    let missing = MemoryId::now_v7();
    assert!(!port.update(&missing, "new").await.unwrap());
    assert!(!port.delete(&missing).await.unwrap());
    assert!(!port.pin(&missing, true).await.unwrap());
    assert!(!port.mark_outdated(&missing).await.unwrap());

    let item = entry("one", "query purity", 100);
    port.write(item).await.unwrap();
    let revision = port.revision();
    let _ = port.list(None);
    let _ = port.stats();
    let _ = port.retrieve_for_inject(&MemoryQuery {
        limit: 1,
        layer: None,
        category: None,
        now: 100,
    });
    let _ = port.search(&MemorySearchQuery {
        text: "query".to_string(),
        limit: 1,
        layer: None,
        category: None,
        include_archive: true,
        now: 100,
    });
    assert_eq!(port.revision(), revision);
}
