//! Contract for reflection-driven synthesis (#1776).
//!
//! Reflection also answers "can these memories be combined into a conclusion?"
//! A synthesized entry stays a plain `MemoryEntry` — it is marked
//! `kind = Synthesized` and carries its sources in `evidence`. Fewer than two
//! sources is not a synthesis, it is a copy (M13), so such a suggestion must
//! never produce one.

use memory::api::reflection::*;
use memory::api::*;

fn now() -> u64 {
    1_000
}

fn entry(content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        now(),
        MemoryLayer::Project,
        MemoryCategory::Decision,
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

fn suggestion(content: &str) -> MemorySuggestion {
    MemorySuggestion {
        layer: MemoryLayer::Project,
        category: MemoryCategory::Decision,
        content: content.to_string(),
        tags: vec!["reflection".to_string()],
        reason: "combines two facts".to_string(),
        supersedes: Vec::new(),
        synthesizes: Vec::new(),
    }
}

fn synthesizing(content: &str, sources: Vec<MemoryId>) -> MemorySuggestion {
    MemorySuggestion {
        synthesizes: sources,
        ..suggestion(content)
    }
}

async fn project_entries(memory: &InMemoryMemory) -> Vec<MemoryEntry> {
    memory.list(Some(MemoryLayer::Project)).await
}

async fn find_by_content(memory: &InMemoryMemory, needle: &str) -> MemoryEntry {
    project_entries(memory)
        .await
        .into_iter()
        .find(|stored| stored.content.contains(needle))
        .unwrap_or_else(|| panic!("no entry containing {needle}"))
}

// --- L1: mapping and serde ---------------------------------------------------

/// A synthesized suggestion maps to `kind = Synthesized` plus its sources as
/// `evidence`; the sources stay untouched in active.
#[tokio::test]
async fn a_synthesis_carries_its_sources_as_evidence() {
    let memory = port();
    let deploy = entry("the deploy target is the staging cluster");
    let window = entry("the deploy window is the tuesday night batch");
    memory.write(deploy.clone()).await.unwrap();
    memory.write(window.clone()).await.unwrap();

    // Wording deliberately shares no tokens with either source: a conclusion
    // that reads like a source would be dedup-merged instead of stored.
    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![synthesizing(
                "release ownership and shipping cadence belong to one platform team",
                vec![deploy.id, window.id],
            )],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    let conclusion = find_by_content(&memory, "release ownership").await;
    assert_eq!(conclusion.kind, MemoryKind::Synthesized);
    assert_eq!(conclusion.evidence.len(), 2);
    assert!(conclusion.evidence.contains(&deploy.id));
    assert!(conclusion.evidence.contains(&window.id));

    for source in [deploy.id, window.id] {
        let stored = project_entries(&memory)
            .await
            .into_iter()
            .find(|stored| stored.id == source)
            .expect("the source stays in active");
        assert_eq!(
            stored.kind,
            MemoryKind::Raw,
            "sources are not re-typed by being cited"
        );
        assert!(stored.evidence.is_empty());
    }
}

/// M13: a single source is a copy, not a synthesis. The entry is still written
/// — the content is useful — but it stays `Raw` with no evidence.
#[tokio::test]
async fn a_single_source_never_produces_a_synthesized_entry() {
    let memory = port();
    let only_source = entry("the deploy target is the staging cluster");
    memory.write(only_source.clone()).await.unwrap();

    let result = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![synthesizing(
                "staging deployments now require a written approval step",
                vec![only_source.id],
            )],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    assert_eq!(result.suggestions_added, 1, "the content is still written");
    let restated = find_by_content(&memory, "written approval step").await;
    assert_eq!(
        restated.kind,
        MemoryKind::Raw,
        "M13 forbids a one-source synthesis"
    );
    assert!(restated.evidence.is_empty());
}

/// An empty source list is the ordinary case (rollback path).
#[tokio::test]
async fn a_suggestion_without_sources_stays_raw() {
    let memory = port();
    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion("the release checklist lives in the wiki")],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    let entry = find_by_content(&memory, "release checklist").await;
    assert_eq!(entry.kind, MemoryKind::Raw);
    assert!(entry.evidence.is_empty());
}

/// L1: the prompt contract parses both with and without the new field.
#[test]
fn the_synthesizes_field_parses_with_and_without_legacy_output() {
    let with_field = r#"{"deviations":[],"suggested_memories":[{"layer":"project","category":"decision","content":"c","tags":[],"reason":"r","synthesizes":["00000000-0000-0000-0000-000000000001"]}],"outdated_memories":[]}"#;
    let legacy = r#"{"deviations":[],"suggested_memories":[{"layer":"project","category":"decision","content":"c","tags":[],"reason":"r"}],"outdated_memories":[]}"#;

    let parsed = serde_json::from_str::<ReflectionOutput>(with_field).expect("must parse");
    assert_eq!(parsed.suggested_memories[0].synthesizes.len(), 1);

    let parsed = serde_json::from_str::<ReflectionOutput>(legacy).expect("must parse");
    assert!(
        parsed.suggested_memories[0].synthesizes.is_empty(),
        "a model that ignores the new field degrades to the old behaviour"
    );
}

// --- L2: coexistence and lifecycle -------------------------------------------

/// A suggestion may both synthesize and supersede; the two relations stay
/// independent.
#[tokio::test]
async fn a_suggestion_can_synthesize_and_supersede_at_once() {
    let memory = port();
    let old = entry("the deploy target is the staging cluster");
    let other = entry("the deploy window is the tuesday night batch");
    memory.write(old.clone()).await.unwrap();
    memory.write(other.clone()).await.unwrap();

    let result = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![MemorySuggestion {
                supersedes: vec![old.id],
                ..synthesizing(
                    "release ownership and shipping cadence belong to one platform team",
                    vec![old.id, other.id],
                )
            }],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    assert_eq!(result.superseded, 1, "the replace relation is established");
    assert_eq!(result.suggestions_added, 1);

    let conclusion = find_by_content(&memory, "release ownership").await;
    assert_eq!(conclusion.kind, MemoryKind::Synthesized);
    assert_eq!(conclusion.evidence.len(), 2);
    assert!(project_entries(&memory)
        .await
        .iter()
        .any(|stored| stored.id == old.id && stored.superseded_by == Some(conclusion.id)));
}

/// Rollback: a model that stops emitting `synthesizes` returns to the previous
/// behaviour with no residue from the synthesis path.
#[tokio::test]
async fn stopping_the_synthesizes_output_degrades_to_the_previous_behaviour() {
    let memory = port();
    let source = entry("the deploy target is the staging cluster");
    memory.write(source.clone()).await.unwrap();

    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion("the deploy target is read from the wiki")],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    let written = find_by_content(&memory, "read from the wiki").await;
    assert_eq!(written.kind, MemoryKind::Raw);
    assert!(written.evidence.is_empty());
    assert!(project_entries(&memory)
        .await
        .iter()
        .all(|stored| stored.kind == MemoryKind::Raw));
}

/// M11 keeps holding for synthesis: every source must already exist.
#[tokio::test]
async fn a_synthesis_citing_unknown_sources_is_rejected() {
    let memory = port();

    let result = memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![synthesizing(
                "a conclusion with no traceable sources",
                vec![MemoryId::now_v7(), MemoryId::now_v7()],
            )],
            ..ReflectionOutput::default()
        })
        .await;

    assert!(
        matches!(result, Err(MemoryError::InvalidEntry { .. })),
        "sources that do not exist must be rejected: {result:?}"
    );
    assert!(
        project_entries(&memory).await.is_empty(),
        "nothing was written"
    );
}

// --- L4: scenario ------------------------------------------------------------

/// L4: several related memories → one reflection → a conclusion whose evidence
/// walks back to every source, and the sources remain retrievable.
#[tokio::test]
async fn a_synthesized_conclusion_is_traceable_back_to_every_source() {
    let memory = port();
    let deploy = entry("the deploy target is the staging cluster");
    let window = entry("the deploy window is the tuesday night batch");
    let owner = entry("the staging cluster is owned by the release team");
    for source in [&deploy, &window, &owner] {
        memory.write(source.clone()).await.unwrap();
    }

    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![synthesizing(
                "release ownership and shipping cadence belong to one platform team",
                vec![deploy.id, window.id, owner.id],
            )],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    let conclusion = find_by_content(&memory, "release ownership").await;
    assert_eq!(conclusion.kind, MemoryKind::Synthesized);
    assert_eq!(conclusion.evidence.len(), 3);

    // Every source is still listed and still injected on its own terms.
    let active = project_entries(&memory).await;
    for source in [&deploy, &window, &owner] {
        assert!(
            active.iter().any(|stored| stored.id == source.id),
            "source {} stays in the active set",
            source.id
        );
    }
    assert!(active.iter().any(|stored| stored.id == conclusion.id));
    assert_eq!(active.len(), 4, "three sources plus the conclusion");
}
