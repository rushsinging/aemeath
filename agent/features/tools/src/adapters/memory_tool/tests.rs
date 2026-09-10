use super::helpers::*;
use super::*;
use crate::domain::types::{
    MemoryCategoryInput, MemoryLayerInput, MemoryLocationResult, ToolSchema,
};
use crate::domain::TypedTool;
use memory::api::{
    MemoryCategory, MemoryEntry, MemoryId, MemoryLayer, MemoryPolicy, MemoryPort, MemorySource,
    MemorySuggestion, ReflectionOutput,
};

use std::sync::{Arc, RwLock};

struct SwappableMemorySource {
    current: RwLock<Arc<dyn MemoryPort>>,
}

impl MemoryPortSource for SwappableMemorySource {
    fn current(&self) -> Arc<dyn MemoryPort> {
        self.current.read().unwrap().clone()
    }
}

#[tokio::test]
async fn memory_tool_resolves_current_committed_port_for_each_call() {
    let first = Arc::new(
        memory::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap(),
    );
    first
        .write(test_entry("first committed memory", MemoryCategory::Fact))
        .await
        .unwrap();
    let second = Arc::new(
        memory::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap(),
    );
    second
        .write(test_entry("resumed committed memory", MemoryCategory::Fact))
        .await
        .unwrap();
    let source = Arc::new(SwappableMemorySource {
        current: RwLock::new(first),
    });
    let tool = MemoryTool {
        source: source.clone(),
    };
    let workspace = tempfile::tempdir().unwrap();
    let context = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build();

    let first_result = tool
        .call(
            serde_json::json!({"action": "search", "query": "committed memory"}),
            &context,
        )
        .await;
    *source.current.write().unwrap() = second;
    let resumed_result = tool
        .call(
            serde_json::json!({"action": "search", "query": "committed memory"}),
            &context,
        )
        .await;

    assert!(first_result.text.contains("first committed memory"));
    assert!(!first_result.text.contains("resumed committed memory"));
    assert!(resumed_result.text.contains("resumed committed memory"));
    assert!(!resumed_result.text.contains("first committed memory"));
}

#[test]
fn memory_registry_schema_preserves_action_specific_contract() {
    let registry = crate::adapters::tool_registry::ToolRegistry::new();
    registry.register(MemoryTool {
        source: Arc::new(SwappableMemorySource {
            current: RwLock::new(Arc::new(memory::NoOpMemory)),
        }),
    });

    let descriptor = registry
        .schemas_for("en")
        .into_iter()
        .find(|schema| schema["name"] == "Memory")
        .unwrap();

    assert_eq!(descriptor["input_schema"], memory_input_schema());
    assert_eq!(descriptor["data_schema"], MemoryResult::data_schema());
    assert!(descriptor["description"]
        .as_str()
        .unwrap()
        .contains("automatic injection"));
}

fn test_entry(content: &str, category: MemoryCategory) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        1_000,
        MemoryLayer::Project,
        category,
        content,
        MemorySource::Llm,
    )
    .unwrap()
}

#[tokio::test]
async fn add_result_returns_full_id_for_follow_up_actions() {
    let memory = memory::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let context = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build();

    let result = handlers::add_memory(
        serde_json::json!({
            "action": "add",
            "content": "persist a manageable memory",
            "layer": "project",
            "category": "fact"
        }),
        &context,
        &memory,
    )
    .await;
    let id = result.data.unwrap().id.unwrap();

    assert_eq!(id.len(), 36);
    assert!(result.text.contains(&id));
}

#[tokio::test]
async fn search_result_publishes_ranked_memory_details_for_llm_and_tui() {
    let memory = memory::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
    memory
        .write(test_entry(
            "Rust workspace validation requires cargo clippy",
            MemoryCategory::Pattern,
        ))
        .await
        .unwrap();
    memory
        .write(test_entry(
            "The project uses Rust workspace builds",
            MemoryCategory::Fact,
        ))
        .await
        .unwrap();

    let result = handlers::search_memory(
        serde_json::json!({
            "action": "search",
            "query": "rust clippy",
            "layer": "project",
            "limit": 10
        }),
        &memory,
    );

    assert!(!result.is_error);
    assert!(result.text.contains("cargo clippy"));
    assert!(result.text.contains("project"));
    assert!(result.text.contains("pattern"));
    assert!(result.text.contains("tags="));
    assert!(result.text.contains("relevance"));
    let data = result.data.unwrap();
    let hits = data.hits.unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits[0].content.contains("cargo clippy"));
    assert_eq!(hits[0].layer, MemoryLayerInput::Project);
    assert_eq!(hits[0].category, MemoryCategoryInput::Pattern);
    assert_eq!(hits[0].location, MemoryLocationResult::Active);
    assert!(hits[0].relevance.unwrap() > hits[1].relevance.unwrap());
}

#[tokio::test]
async fn reflection_generated_memory_remains_searchable_through_tool_contract() {
    let memory = memory::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![MemorySuggestion {
                layer: MemoryLayer::Project,
                category: MemoryCategory::Preference,
                content: "Prefer deterministic lexical retrieval".to_string(),
                tags: vec!["reflection".to_string()],
                reason: "user preference".to_string(),
            }],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap();

    let result = handlers::search_memory(
        serde_json::json!({
            "action": "search",
            "query": "deterministic retrieval",
            "category": "preference"
        }),
        &memory,
    );

    let hits = result.data.unwrap().hits.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].category, MemoryCategoryInput::Preference);
    assert_eq!(hits[0].tags, vec!["reflection"]);
}

#[tokio::test]
async fn list_result_publishes_manageable_entries_in_llm_text() {
    let memory = memory::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
    memory
        .write(test_entry(
            "Rust workspace validation requires cargo clippy",
            MemoryCategory::Pattern,
        ))
        .await
        .unwrap();

    let result = handlers::list_memory(serde_json::json!({"action": "list"}), &memory);
    let entries = result.data.unwrap().entries.unwrap();

    assert_eq!(entries.len(), 1);
    assert!(result.text.contains("Rust workspace validation"));
    assert!(result.text.contains("project"));
    assert!(result.text.contains("pattern"));
    assert!(result.text.contains(&entries[0].id));
}

#[tokio::test]
async fn full_add_returns_actionable_typed_eviction_candidates_without_mutation() {
    let memory = memory::InMemoryMemory::new_with_clock(
        MemoryPolicy {
            max_entries: 1,
            similarity_threshold: 0.8,
        },
        || 2_000,
    )
    .unwrap();
    let existing = test_entry("existing capacity candidate", MemoryCategory::Fact);
    memory.write(existing.clone()).await.unwrap();
    let revision = memory.revision();
    let workspace = tempfile::tempdir().unwrap();
    let context = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build();

    let result = handlers::add_memory(
        serde_json::json!({
            "action": "add",
            "content": "a completely unrelated new preference",
            "layer": "project",
            "category": "preference"
        }),
        &context,
        &memory,
    )
    .await;

    assert!(!result.is_error);
    assert_eq!(memory.revision(), revision);
    let data = result.data.unwrap();
    assert_eq!(data.action, "needs_eviction");
    let candidates = data.eviction_candidates.unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, existing.id.to_string());
    assert!(result.text.contains(&candidates[0].id));
    assert!(result.text.contains("archive"));
}

#[tokio::test]
async fn archive_and_restore_actions_publish_manageable_terminal_results() {
    let memory = Arc::new(
        memory::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap(),
    );
    let entry = test_entry("archive lifecycle", MemoryCategory::Decision);
    memory.write(entry.clone()).await.unwrap();
    let source = Arc::new(SwappableMemorySource {
        current: RwLock::new(memory.clone()),
    });
    let tool = MemoryTool { source };
    let workspace = tempfile::tempdir().unwrap();
    let context = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build();

    let archived = tool
        .call(
            serde_json::json!({"action": "archive", "id": entry.id.to_string()}),
            &context,
        )
        .await;
    let restored = tool
        .call(
            serde_json::json!({"action": "restore", "id": entry.id.to_string()}),
            &context,
        )
        .await;

    assert!(!archived.is_error);
    assert_eq!(archived.data.unwrap().action, "archive");
    assert!(!restored.is_error);
    assert_eq!(restored.data.unwrap().action, "restore");
    assert_eq!(memory.list(None), vec![entry]);
}

#[test]
fn memory_schema_publishes_constrained_actions_layers_and_categories() {
    let schema = memory_input_schema();
    let properties = schema["properties"].as_object().unwrap();

    assert_eq!(
        properties["action"]["enum"],
        serde_json::json!([
            "add",
            "delete",
            "search",
            "pin",
            "list",
            "archive",
            "restore",
            "add_reminder",
            "complete_reminder"
        ])
    );
    assert_eq!(
        properties["layer"]["enum"],
        serde_json::json!(["global", "project"])
    );
    assert_eq!(
        properties["category"]["enum"],
        serde_json::json!(["fact", "decision", "preference", "pattern", "pitfall"])
    );
    let action_contracts = schema["oneOf"].as_array().unwrap();
    for (action, required) in [
        ("add", vec!["action", "content"]),
        ("delete", vec!["action", "id"]),
        ("search", vec!["action", "query"]),
        ("pin", vec!["action", "id"]),
        ("list", vec!["action"]),
        ("archive", vec!["action", "id"]),
        ("restore", vec!["action", "id"]),
        ("add_reminder", vec!["action", "content"]),
        ("complete_reminder", vec!["action", "id"]),
    ] {
        let contract = action_contracts
            .iter()
            .find(|contract| contract["properties"]["action"]["const"] == action)
            .unwrap_or_else(|| panic!("missing action contract {action}"));
        assert_eq!(contract["required"], serde_json::json!(required));
    }
}

#[test]
fn memory_description_explains_persistence_layers_categories_and_reminders() {
    let description = share::i18n::tools::core::memory("en").to_lowercase();

    for expected in [
        "persistent",
        "global",
        "project",
        "fact",
        "decision",
        "preference",
        "pattern",
        "pitfall",
        "reminder",
        "automatic injection",
        "search before relying on historical",
        "explicitly asks you to remember",
        "sensitive",
        "must not override system",
        "do not invent",
        "archive",
        "restore",
    ] {
        assert!(
            description.contains(expected),
            "memory description must explain {expected}"
        );
    }
}

#[test]
fn memory_result_schema_exposes_entries_and_search_hits() {
    let schema = MemoryResult::data_schema();
    let properties = schema["properties"].as_object().unwrap();

    assert!(properties.contains_key("id"));
    assert!(properties.contains_key("entries"));
    assert!(properties.contains_key("hits"));
    assert!(properties.contains_key("eviction_candidates"));
    let eviction_properties = properties["eviction_candidates"]["items"]["properties"]
        .as_object()
        .unwrap();
    for field in [
        "id",
        "content",
        "layer",
        "category",
        "tags",
        "pinned",
        "outdated",
        "ttl_expired",
        "confirmation_count",
        "last_confirmed_at",
        "eviction_score",
        "eviction_reason",
    ] {
        assert!(
            eviction_properties.contains_key(field),
            "missing eviction candidate field {field}"
        );
    }
    let hit_properties = properties["hits"]["items"]["properties"]
        .as_object()
        .unwrap();
    for field in [
        "id",
        "content",
        "layer",
        "category",
        "tags",
        "pinned",
        "location",
        "outdated",
        "ttl_expired",
        "relevance",
    ] {
        assert!(
            hit_properties.contains_key(field),
            "missing hit field {field}"
        );
    }
}

#[test]
fn test_validate_content_normal() {
    assert!(validate_content("记住这个决策").is_ok());
}

#[test]
fn test_validate_content_empty() {
    assert!(validate_content("   ").is_err());
}

#[test]
fn test_validate_content_too_long() {
    let content = "x".repeat(MAX_CONTENT_CHARS + 1);
    assert!(validate_content(&content).is_err());
}

#[test]
fn test_parse_tags_normal() {
    let input = serde_json::json!({"tags": ["rust", "rust", " memory "]});
    let tags = parse_tags(&input).unwrap();

    assert_eq!(tags, vec!["memory", "rust"]);
}

#[test]
fn test_parse_tags_empty_array() {
    let input = serde_json::json!({"tags": []});
    let tags = parse_tags(&input).unwrap();

    assert!(tags.is_empty());
}

#[test]
fn test_parse_tags_invalid_item() {
    let input = serde_json::json!({"tags": [1]});

    assert!(parse_tags(&input).is_err());
}
