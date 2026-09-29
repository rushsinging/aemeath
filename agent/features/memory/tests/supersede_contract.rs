//! Contract for the supersede relation (#1774).
//!
//! A memory may name the entry that replaced it. Superseded entries drop out of
//! injection entirely (M10, pinned included) but stay visible to explicit
//! search with their state intact, and no relation may close a cycle (M9).

use memory::api::reflection::*;
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

fn suggestion(content: &str, supersedes: Vec<MemoryId>) -> MemorySuggestion {
    MemorySuggestion {
        layer: MemoryLayer::Project,
        category: MemoryCategory::Fact,
        content: content.to_string(),
        tags: vec!["reflection".to_string()],
        reason: "test".to_string(),
        supersedes,
        synthesizes: Vec::new(),
    }
}

fn port() -> InMemoryMemory {
    InMemoryMemory::new(MemoryPolicy {
        max_entries: 50,
        similarity_threshold: 0.8,
    })
    .expect("policy must be valid")
}

fn superseded_by_of(memory: &InMemoryMemory, id: MemoryId) -> Option<MemoryId> {
    memory
        .list(Some(MemoryLayer::Project))
        .into_iter()
        .find(|stored| stored.id == id)
        .and_then(|stored| stored.superseded_by)
}

fn inject(memory: &dyn MemoryPort) -> Vec<MemoryId> {
    memory
        .retrieve_for_inject(&MemoryQuery {
            limit: 50,
            layer: Some(MemoryLayer::Project),
            category: None,
            now: now(),
        })
        .hits
        .into_iter()
        .map(|hit| hit.entry.id)
        .collect()
}

fn search(memory: &dyn MemoryPort, text: &str) -> Vec<MemorySearchHit> {
    memory
        .search(&MemorySearchQuery {
            text: text.to_string(),
            limit: 50,
            layer: None,
            category: None,
            include_archive: false,
            now: now(),
        })
        .hits
}

/// L2：apply 携带 `supersedes` 时建立单向关系并如实计数。
#[tokio::test]
async fn apply_establishes_the_supersede_relation_and_counts_it() {
    let memory = port();
    let old = entry("the deploy runs on friday");
    memory.write(old.clone()).await.unwrap();

    let result = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion("the deploy now runs on monday", vec![old.id])],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    assert_eq!(result.suggestions_added, 1);
    assert_eq!(result.superseded, 1, "one relation was established");
    assert_eq!(superseded_by_of(&memory, old.id), {
        // The relation points at the entry that survived the apply.
        let replacement = memory
            .list(Some(MemoryLayer::Project))
            .into_iter()
            .find(|stored| stored.content == "the deploy now runs on monday")
            .expect("replacement entry");
        Some(replacement.id)
    });
}

/// L2：一条建议可以同时取代多条。
#[tokio::test]
async fn one_suggestion_can_supersede_several_entries() {
    let memory = port();
    let first = entry("the api port is 8080");
    let second = entry("the api port is 9090");
    memory.write(first.clone()).await.unwrap();
    memory.write(second.clone()).await.unwrap();

    let result = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion(
                "the api port comes from config",
                vec![first.id, second.id],
            )],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    assert_eq!(result.superseded, 2);
    let replacement = memory
        .list(Some(MemoryLayer::Project))
        .into_iter()
        .find(|stored| stored.content == "the api port comes from config")
        .expect("replacement entry")
        .id;
    assert_eq!(superseded_by_of(&memory, first.id), Some(replacement));
    assert_eq!(superseded_by_of(&memory, second.id), Some(replacement));
}

/// M9：会成环的关系被跳过，同批次先建立的合法关系不受影响——不半写入。
///
/// 环只能经由「合并」产生：新条目的 id 每次都是新生成的，只有当它被并入
/// 已有条目时，留存 id 才可能落回链上已有节点，从而闭合回路。
#[tokio::test]
async fn a_relation_that_would_close_a_cycle_is_skipped_without_blocking_the_batch() {
    let memory = port();
    let ancestor = entry("the schema version is one");
    let unrelated = entry("the release train is weekly");
    memory.write(ancestor.clone()).await.unwrap();
    memory.write(unrelated.clone()).await.unwrap();

    // v2 取代 v1：v1 的上游变成 v2。
    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion("the schema version is two", vec![ancestor.id])],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();
    let v2 = memory
        .list(Some(MemoryLayer::Project))
        .into_iter()
        .find(|stored| stored.content == "the schema version is two")
        .expect("v2 entry")
        .id;
    assert_eq!(superseded_by_of(&memory, ancestor.id), Some(v2));

    // 内容与 v1 相同 → 合并，留存 id 回到 v1；v1 再取代 v2 即闭合 v1 <- v2 <- v1。
    let cyclic = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion("the schema version is one", vec![v2])],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();
    assert_eq!(
        cyclic.superseded, 0,
        "a cyclic relation must not be written"
    );
    assert_eq!(
        superseded_by_of(&memory, v2),
        None,
        "the rejected relation left no trace on the target"
    );
    assert_eq!(
        superseded_by_of(&memory, ancestor.id),
        Some(v2),
        "the relation established earlier survives the rejection"
    );

    // 合法关系照常建立。
    let legal = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion("the release train is daily", vec![unrelated.id])],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();
    assert_eq!(legal.superseded, 1);
    assert!(!inject(&memory).contains(&ancestor.id), "M10 applies to v1");
}

/// 合并会让关系指向被取代条目自身，形成自环——M9 因此拒绝该关系。
/// 这不是缺陷：合并说明两条内容等价，「用新条目取代旧条目」此时没有意义。
#[tokio::test]
async fn a_suggestion_merged_into_its_own_target_yields_no_relation() {
    let memory = port();
    let old = entry("retry the flaky upload once");
    memory.write(old.clone()).await.unwrap();

    // 内容几乎相同 → 合并进 old，留存 id 就是被取代目标自身。
    let result = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion("retry the flaky upload once", vec![old.id])],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    assert_eq!(
        result.suggestions_added, 1,
        "the merge still counts as applied"
    );
    assert_eq!(
        result.superseded, 0,
        "a self-reference is a cycle and must be rejected"
    );
    assert_eq!(superseded_by_of(&memory, old.id), None);
    assert!(inject(&memory).contains(&old.id), "M10 does not apply here");
}

/// M10：被取代条目不再注入，pinned 也不例外。
#[tokio::test]
async fn a_pinned_superseded_entry_still_leaves_the_injection_set() {
    let memory = port();
    let mut pinned = entry("never evict the release checklist");
    pinned.pinned = true;
    memory.write(pinned.clone()).await.unwrap();
    assert!(inject(&memory).contains(&pinned.id), "baseline");

    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion(
                "the release checklist moved to the wiki",
                vec![pinned.id],
            )],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    assert!(
        !inject(&memory).contains(&pinned.id),
        "pinned protects against eviction, not against being superseded"
    );
}

/// L4：场景——取代后不再注入，但显式检索仍能看见它并读到取代状态。
#[tokio::test]
async fn a_superseded_entry_stays_searchable_with_its_supersede_state() {
    let memory = port();
    let old = entry("the staging host is alpha");
    memory.write(old.clone()).await.unwrap();
    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion("the staging host is beta", vec![old.id])],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    assert!(
        !inject(&memory).contains(&old.id),
        "the superseded entry must leave injection"
    );

    let hits = search(&memory, "staging host");
    let hit = hits
        .iter()
        .find(|hit| hit.entry.id == old.id)
        .expect("explicit search must still surface the entry");
    assert_eq!(
        hit.superseded_by,
        superseded_by_of(&memory, old.id),
        "the hit carries the same relation the store holds"
    );
    assert!(hit.superseded_by.is_some());

    let replacement = hit
        .superseded_by
        .expect("the relation names the replacement");
    assert!(
        inject(&memory).contains(&replacement),
        "the replacement is what injection now serves"
    );
}

/// L3：NoOpMemory 不建立任何关系，计数为零。
#[tokio::test]
async fn the_noop_port_reports_zero_supersede_relations() {
    let memory = NoOpMemory;

    let result = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion("anything", vec![MemoryId::now_v7()])],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    assert_eq!(result.suggestions_added, 0);
    assert_eq!(result.superseded, 0);
}

/// 兼容：LLM 旧输出（没有 `supersedes`）照常解析，关系为空。
#[test]
fn a_suggestion_without_the_supersede_field_parses_with_no_relations() {
    let output: ReflectionOutput = serde_json::from_str(
        r#"{"deviations":[],"suggested_memories":[{"layer":"project","category":"fact","content":"legacy","tags":[],"reason":"r"}],"outdated_memories":[]}"#,
    )
    .expect("legacy reflection output must parse");

    assert!(output.suggested_memories[0].supersedes.is_empty());
}
