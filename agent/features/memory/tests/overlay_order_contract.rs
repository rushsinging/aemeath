//! Contract for the overlay ordering of synthesized conclusions (#1777).
//!
//! Injection is budget-tight, so a conclusion and the facts it cites must not
//! both occupy the budget. The rule is *overlay*, not downweighting: the
//! conclusion is offered first, and the facts it covers step aside **only when
//! the conclusion actually made it in**. A conclusion that loses the budget
//! contest must not take its sources down with it.

use memory::api::reflection::ReflectionOutput;
use memory::api::search::*;
use memory::api::*;

fn now() -> u64 {
    1_000
}

fn raw_entry(content: &str, confirmed: u32) -> MemoryEntry {
    let mut entry = MemoryEntry::new(
        MemoryId::now_v7(),
        now(),
        MemoryLayer::Project,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .expect("entry must be valid");
    entry.confirmation_count = confirmed;
    entry
}

fn port() -> InMemoryMemory {
    InMemoryMemory::new(MemoryPolicy {
        max_entries: 50,
        similarity_threshold: 0.8,
    })
    .expect("policy must be valid")
}

async fn injected_order(memory: &InMemoryMemory) -> Vec<MemoryId> {
    memory
        .retrieve_for_inject(&MemoryQuery {
            limit: 50,
            layer: Some(MemoryLayer::Project),
            category: None,
            now: now(),
        })
        .await
        .hits
        .into_iter()
        .map(|hit| hit.entry.id)
        .collect()
}

fn position_of(order: &[MemoryId], id: MemoryId) -> usize {
    order
        .iter()
        .position(|candidate| *candidate == id)
        .unwrap_or_else(|| panic!("entry {id} is not in the injection set"))
}

/// The synthesized conclusion is offered ahead of the facts it cites, even
/// though its score is lower — the overlay does not touch the score, only the
/// order in which candidates are offered.
#[tokio::test]
async fn a_synthesized_conclusion_is_offered_before_the_facts_it_covers() {
    let memory = port();
    // A heavily confirmed fact would outrank a fresh conclusion on score alone.
    let fact = raw_entry("the deploy target is the staging cluster", 9);
    let other = raw_entry("the deploy window is the tuesday night batch", 9);
    memory.write(fact.clone()).await.unwrap();
    memory.write(other.clone()).await.unwrap();
    // The conclusion is written through reflection so it keeps its kind.
    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![memory::api::MemorySuggestion {
                layer: MemoryLayer::Project,
                category: MemoryCategory::Decision,
                content: "release ownership and shipping cadence belong to one team".to_string(),
                tags: vec![],
                reason: "combines facts".to_string(),
                supersedes: vec![],
                synthesizes: vec![fact.id, other.id],
            }],
            ..Default::default()
        })
        .await
        .unwrap();

    let order = injected_order(&memory).await;
    let conclusion_at = position_of(&order, conclusion_of(&memory).await);
    let fact_at = position_of(&order, fact.id);
    assert!(
        conclusion_at < fact_at,
        "the conclusion must be offered first: {order:?}"
    );
}

/// Every synthesized entry leads, not just the one that happens to rank first.
#[tokio::test]
async fn all_synthesized_entries_lead_the_injection_order() {
    let memory = port();
    let mut sources = Vec::new();
    for index in 0..3 {
        let fact = raw_entry(&format!("the historical fact number {index} holds"), 9);
        memory.write(fact.clone()).await.unwrap();
        sources.push(fact.id);
    }

    let mut conclusions = Vec::new();
    for index in 0..2 {
        memory
            .apply_reflection(&ReflectionOutput {
                suggested_memories: vec![memory::api::MemorySuggestion {
                    layer: MemoryLayer::Project,
                    category: MemoryCategory::Pattern,
                    content: format!("the consolidated conclusion number {index} holds"),
                    tags: vec![],
                    reason: "combines facts".to_string(),
                    supersedes: vec![],
                    synthesizes: sources.clone(),
                }],
                ..Default::default()
            })
            .await
            .unwrap();
        conclusions.push(conclusion_of(&memory).await);
    }

    let order = injected_order(&memory).await;
    let last_conclusion = conclusions
        .iter()
        .map(|id| position_of(&order, *id))
        .max()
        .expect("two conclusions were written");
    let first_fact = sources
        .iter()
        .map(|id| position_of(&order, *id))
        .min()
        .expect("three facts were written");
    assert!(
        last_conclusion < first_fact,
        "every conclusion precedes every fact: {order:?}"
    );
}

/// Score order is untouched within each group — the overlay only groups.
#[tokio::test]
async fn score_order_is_preserved_inside_each_group() {
    let memory = port();
    let mut low = raw_entry("a rarely confirmed note about the pipeline", 0);
    let mut high = raw_entry("a heavily confirmed note about the pipeline", 9);
    low.confirmation_count = 0;
    high.confirmation_count = 9;
    memory.write(low.clone()).await.unwrap();
    memory.write(high.clone()).await.unwrap();

    let order = injected_order(&memory).await;
    assert!(
        position_of(&order, high.id) < position_of(&order, low.id),
        "score ordering still decides within the group: {order:?}"
    );
}

/// Explicit search is the "look up the details" path: nothing steps aside.
#[tokio::test]
async fn explicit_search_keeps_the_conclusion_and_its_sources_side_by_side() {
    let memory = port();
    let fact = raw_entry("the deploy target is the staging cluster", 9);
    let other = raw_entry("the deploy window is the tuesday night batch", 9);
    memory.write(fact.clone()).await.unwrap();
    memory.write(other.clone()).await.unwrap();
    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![memory::api::MemorySuggestion {
                layer: MemoryLayer::Project,
                category: MemoryCategory::Decision,
                content: "release ownership and shipping cadence belong to one team".to_string(),
                tags: vec![],
                reason: "combines facts".to_string(),
                supersedes: vec![],
                synthesizes: vec![fact.id, other.id],
            }],
            ..Default::default()
        })
        .await
        .unwrap();

    // Each side is reachable through its own wording — search never trades
    // detail reachability for budget.

    assert!(
        search_project_hits(&memory, "deploy")
            .await
            .iter()
            .any(|hit| hit.entry.id == fact.id),
        "the source stays reachable"
    );
    assert!(
        search_project_hits(&memory, "ownership")
            .await
            .iter()
            .any(|hit| hit.entry.kind == MemoryKind::Synthesized),
        "the conclusion stays reachable"
    );
}

async fn conclusion_of(memory: &InMemoryMemory) -> MemoryId {
    memory
        .list(Some(MemoryLayer::Project))
        .await
        .into_iter()
        .find(|entry| entry.kind == MemoryKind::Synthesized)
        .expect("a synthesized entry was written")
        .id
}

/// 项目层词法搜索的命中集（async port 签名迁移后的测试辅助）。
async fn search_project_hits(memory: &InMemoryMemory, text: &str) -> Vec<MemorySearchHit> {
    memory
        .search(&MemorySearchQuery {
            text: text.to_string(),
            limit: 50,
            layer: Some(MemoryLayer::Project),
            category: None,
            include_archive: false,
            now: now(),
        })
        .await
        .hits
}
