//! Contract for evidence pointers and the memory `kind` (#1775).
//!
//! A dedup hit no longer destroys the incoming entry: it is archived on the
//! same layer and the surviving entry keeps a pointer to it (M11). Every
//! pointer must resolve; compaction must not dangle them. `kind` states where
//! an entry came from — merging and synthesis both fill `evidence`, but their
//! read-side behaviour differs, so the type is explicit rather than derived.

use memory::api::search::*;
use memory::api::*;

fn now() -> u64 {
    1_000
}

fn entry(content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        now(),
        MemoryLayer::Project,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .expect("entry must be valid")
}

fn port() -> InMemoryMemory {
    InMemoryMemory::new(MemoryPolicy {
        max_entries: 50,
        similarity_threshold: 0.8,
    })
    .expect("policy must be valid")
}

async fn active_entries(memory: &InMemoryMemory) -> Vec<MemoryEntry> {
    memory.list(Some(MemoryLayer::Project)).await
}

/// 列出 archive 条目。`search` 是唯一能触达 archive 的读取口（`list` 只返回
/// active），且空查询不产生 hit，因此必须给内容词。
async fn archived_entries(memory: &InMemoryMemory, text: &str) -> Vec<MemoryEntry> {
    memory
        .search(&MemorySearchQuery {
            text: text.to_string(),
            limit: 50,
            layer: Some(MemoryLayer::Project),
            category: None,
            include_archive: true,
            now: now(),
        })
        .await
        .hits
        .into_iter()
        .filter(|hit| hit.location == MemoryLocation::Archive)
        .map(|hit| hit.entry)
        .collect()
}

// --- L1: defaults and serde -------------------------------------------------

/// `kind` defaults to `Raw` and `evidence` starts empty.
#[test]
fn a_fresh_entry_is_raw_with_no_evidence() {
    let entry = entry("plain fact");
    assert_eq!(entry.kind, MemoryKind::Raw);
    assert!(entry.evidence.is_empty());
}

/// Old persisted entries lack both fields: they decode as `Raw` / empty and
/// keep injecting eligibility.
#[test]
fn a_persisted_entry_without_the_new_fields_loads_with_safe_defaults() {
    let legacy = r#"{
        "id": "00000000-0000-7000-8000-000000000001",
        "layer": "project",
        "category": "fact",
        "content": "legacy entry",
        "source": "llm",
        "created_at": 10,
        "last_confirmed_at": 10
    }"#;

    let parsed: MemoryEntry = serde_json::from_str(legacy).expect("legacy entry must decode");
    assert_eq!(parsed.kind, MemoryKind::Raw);
    assert!(parsed.evidence.is_empty());
}

/// Round trip keeps the kind and the pointer list; the default form stays
/// absent from the payload.
#[test]
fn kind_and_evidence_round_trip() {
    let mut synthesized = entry("the schema is v2");
    synthesized.kind = MemoryKind::Synthesized;
    synthesized.evidence = vec![MemoryId::now_v7(), MemoryId::now_v7()];

    let encoded = serde_json::to_value(&synthesized).expect("entry must encode");
    assert_eq!(encoded["kind"], serde_json::json!("synthesized"));
    assert_eq!(encoded["evidence"].as_array().map(Vec::len), Some(2));
    let decoded: MemoryEntry = serde_json::from_value(encoded).expect("entry must decode");
    assert_eq!(decoded, synthesized);

    let plain = serde_json::to_value(entry("plain")).expect("entry must encode");
    assert!(
        plain.get("kind").is_none(),
        "Raw must not occupy the payload"
    );
    assert!(
        plain.get("evidence").is_none(),
        "empty evidence must not occupy the payload"
    );
}

// --- L2: the merge path ------------------------------------------------------

/// A dedup hit keeps the existing entry active, archives the incoming one, and
/// records the pointer on the survivor. Confirmation counting is unchanged.
#[tokio::test]
async fn a_merge_archives_the_incoming_entry_and_records_the_pointer() {
    let memory = port();
    let existing = entry("the deploy target is the staging cluster");
    memory.write(existing.clone()).await.unwrap();

    // Jaccard({…,today} vs {…}) = 7/8 >= 0.8: the dedup threshold is met.
    let incoming = entry("the deploy target is the staging cluster today");
    let result = memory.write(incoming.clone()).await.unwrap();

    let WriteResult::Merged { existing_id } = result else {
        panic!("a dedup hit must merge, got {result:?}");
    };
    assert_eq!(existing_id, existing.id, "the incumbent stays active");

    let active = active_entries(&memory).await;
    assert_eq!(active.len(), 1, "the active set must not grow");
    assert_eq!(active[0].id, existing.id);
    assert_eq!(active[0].evidence, vec![incoming.id]);
    assert_eq!(
        active[0].confirmation_count, 1,
        "confirmation counting is unchanged"
    );

    let archived = archived_entries(&memory, "deploy").await;
    assert_eq!(
        archived.len(),
        1,
        "the incoming entry is archived, not dropped"
    );
    assert_eq!(archived[0].id, incoming.id);
    assert_eq!(
        archived[0].content, incoming.content,
        "content survives intact"
    );
    assert_eq!(archived[0].evidence, Vec::<MemoryId>::new());
}

/// Writes that miss the dedup threshold behave exactly as before.
#[tokio::test]
async fn a_write_without_a_dedup_hit_stays_unchanged() {
    let memory = port();
    let first = entry("the deploy runs on friday");
    memory.write(first.clone()).await.unwrap();

    let unrelated = entry("the billing provider is stripe");
    let result = memory.write(unrelated.clone()).await.unwrap();
    assert!(matches!(result, WriteResult::Added { .. }));

    let active = active_entries(&memory).await;
    assert_eq!(active.len(), 2);
    assert!(active.iter().all(|stored| stored.evidence.is_empty()));
    assert!(archived_entries(&memory, "deploy").await.is_empty());
}

/// Repeated merges accumulate pointers in write order.
#[tokio::test]
async fn repeated_merges_accumulate_evidence_in_order() {
    let memory = port();
    let survivor = entry("the deploy target is the staging cluster");
    memory.write(survivor.clone()).await.unwrap();
    for suffix in ["today", "tonight"] {
        memory
            .write(entry(&format!(
                "the deploy target is the staging cluster {suffix}"
            )))
            .await
            .unwrap();
    }

    let active = active_entries(&memory).await;
    assert_eq!(active.len(), 1);
    assert_eq!(
        active[0].evidence.len(),
        2,
        "both merged sources are tracked"
    );
}

/// M11: an entry whose evidence names entries the store has never seen is
/// rejected on write — pointers must resolve from day one.
#[tokio::test]
async fn a_write_with_dangling_evidence_is_rejected() {
    let memory = port();

    let mut dangling = entry("a claim citing unknown sources");
    dangling.evidence = vec![MemoryId::now_v7()];

    let error = memory.write(dangling).await.unwrap_err();
    assert!(
        matches!(error, MemoryError::InvalidEntry { .. }),
        "dangling evidence must be rejected: {error:?}"
    );
    assert!(
        active_entries(&memory).await.is_empty(),
        "nothing was written"
    );
}

/// Evidence may legitimately point at entries that already exist on the layer.
#[tokio::test]
async fn a_write_with_resolvable_evidence_is_accepted() {
    let memory = port();
    let source = entry("the staging host is alpha");
    memory.write(source.clone()).await.unwrap();

    let mut derived = entry("a note referencing the staging host");
    derived.evidence = vec![source.id];
    memory.write(derived).await.unwrap();

    let active = active_entries(&memory).await;
    assert_eq!(active.len(), 2);
    assert_eq!(active[1].evidence, vec![source.id]);
}

/// M11 across compaction: a compact must never leave an evidence pointer
/// dangling. Today's compaction only archives actives (it never deletes), so
/// the merged source always survives — this test pins that invariant so a
/// future archive-eviction feature cannot silently break it.
#[tokio::test]
async fn compaction_never_dangles_an_evidence_pointer() {
    let memory = port();
    let survivor = entry("the deploy target is the staging cluster");
    memory.write(survivor.clone()).await.unwrap();
    memory
        .write(entry("the deploy target is the staging cluster today"))
        .await
        .unwrap();
    for index in 0..5 {
        memory
            .write(entry(&format!(
                "unrelated fact number {index} with padding"
            )))
            .await
            .unwrap();
    }

    memory.compact().await.unwrap();

    let active = active_entries(&memory).await;
    let archived = archived_entries(&memory, "deploy").await;
    for stored in &active {
        for pointer in &stored.evidence {
            assert!(
                archived.iter().any(|entry| &entry.id == pointer)
                    || active.iter().any(|entry| &entry.id == pointer),
                "pointer {pointer} must still resolve after compaction"
            );
        }
    }
    assert!(
        archived
            .iter()
            .any(|entry| entry.content.contains("today") && entry.evidence.is_empty()),
        "the merged source survives compaction with its content intact"
    );
}

/// Restoring a referenced archive entry keeps every pointer resolvable: the id
/// and content are unchanged, so evidence still resolves — now against active.
#[tokio::test]
async fn restoring_a_referenced_entry_keeps_the_pointers_valid() {
    let memory = port();
    let survivor = entry("the deploy target is the staging cluster");
    memory.write(survivor.clone()).await.unwrap();
    let incoming = entry("the deploy target is the staging cluster today");
    memory.write(incoming.clone()).await.unwrap();

    let restored = memory.restore(&incoming.id).await.unwrap();
    assert!(
        matches!(restored, RestoreResult::Restored { id } if id == incoming.id),
        "a referenced archive entry can be restored"
    );
    let active = active_entries(&memory).await;
    assert!(
        active
            .iter()
            .any(|stored| stored.evidence.contains(&incoming.id)),
        "the survivor still holds the pointer"
    );
    assert!(
        active.iter().any(|stored| stored.id == incoming.id),
        "the restored entry now lives in active, where the pointer resolves"
    );
    assert!(
        archived_entries(&memory, "deploy").await.is_empty(),
        "restoring removed the entry from the archive without breaking anything"
    );
}

/// History is never backfilled: entries merged before the change carry no
/// pointers, and nothing may fabricate them.
#[test]
fn historical_merges_are_never_backfilled() {
    let legacy = r#"{
        "id": "00000000-0000-7000-8000-000000000002",
        "layer": "project",
        "category": "fact",
        "content": "merged long ago",
        "source": "llm",
        "created_at": 10,
        "last_confirmed_at": 10,
        "confirmation_count": 7
    }"#;
    let parsed: MemoryEntry = serde_json::from_str(legacy).expect("legacy entry must decode");
    assert!(
        parsed.evidence.is_empty() && parsed.confirmation_count == 7,
        "a high confirmation count with no evidence is history, not a gap to fill"
    );
}

/// NoOpMemory never produces evidence-bearing writes.
#[tokio::test]
async fn the_noop_port_keeps_its_write_shape() {
    let memory = NoOpMemory;
    let result = memory.write(entry("anything")).await.unwrap();
    assert_eq!(result, WriteResult::NoOp);
}
