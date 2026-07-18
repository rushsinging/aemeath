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

fn suggestion(layer: MemoryLayer, content: &str) -> MemorySuggestion {
    MemorySuggestion {
        layer,
        category: MemoryCategory::Fact,
        content: content.to_string(),
        tags: vec!["reflection".to_string()],
        reason: "test".to_string(),
    }
}

#[tokio::test]
async fn reflection_adds_and_merges_suggestions_with_injected_time() {
    let port = InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 4242).unwrap();
    let output = ReflectionOutput {
        suggested_memories: vec![
            suggestion(MemoryLayer::Project, "Reflection contract"),
            suggestion(MemoryLayer::Project, "reflection contract"),
        ],
        ..ReflectionOutput::default()
    };

    let result = port.apply_reflection(&output).await.unwrap();

    assert_eq!(result.suggestions_added, 2);
    let entries = port.list(Some(MemoryLayer::Project));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].created_at, 4242);
    assert_eq!(entries[0].accessed_at, 4242);
    assert_eq!(entries[0].access_count, 1);
    assert_eq!(entries[0].id.as_uuid().get_version_num(), 7);
}

#[tokio::test]
async fn reflection_evicts_unpinned_candidate_and_retries_once() {
    let port = InMemoryMemory::new_with_clock(
        MemoryPolicy {
            max_entries: 1,
            similarity_threshold: 0.8,
        },
        || 200,
    )
    .unwrap();
    let old = entry("old", "old unrelated fact", 100);
    port.write(old.clone()).await.unwrap();

    let result = port
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion(MemoryLayer::Project, "brand new decision")],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    assert_eq!(result.suggestions_added, 1);
    assert_eq!(port.stats().project_archive_count, 1);
    assert_eq!(
        port.list(Some(MemoryLayer::Project))[0].content,
        "brand new decision"
    );
}

#[tokio::test]
async fn reflection_reports_full_capacity_when_only_pinned_entries_exist() {
    let port = InMemoryMemory::new_with_clock(
        MemoryPolicy {
            max_entries: 1,
            similarity_threshold: 0.8,
        },
        || 200,
    )
    .unwrap();
    let mut pinned = entry("pinned", "pinned old fact", 100);
    pinned.pinned = true;
    port.write(pinned.clone()).await.unwrap();

    let error = port
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion(MemoryLayer::Project, "cannot fit")],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap_err();

    assert!(
        matches!(error, MemoryError::InvalidEntry { message } if message.contains("淘汰") && message.contains("容量"))
    );
    assert_eq!(port.list(None), vec![pinned]);
}

#[tokio::test]
async fn reflection_marks_existing_outdated_ids_and_rejects_invalid_ids() {
    let port = InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 200).unwrap();
    let existing = entry("existing", "existing fact", 100);
    port.write(existing.clone()).await.unwrap();

    let result = port
        .apply_reflection(&ReflectionOutput {
            outdated_memories: vec![existing.id.to_string(), MemoryId::now_v7().to_string()],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();
    assert_eq!(result.outdated_marked, 1);
    assert!(port.list(None)[0].outdated);

    let error = port
        .apply_reflection(&ReflectionOutput {
            outdated_memories: vec!["not-a-uuid".to_string()],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap_err();
    assert!(matches!(error, MemoryError::InvalidEntry { .. }));
}
