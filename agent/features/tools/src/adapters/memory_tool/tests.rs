use super::constants::MAX_CONTENT_CHARS;
use super::helpers::*;
use super::*;
use crate::adapters::memory_tool::{
    MemoryAddTool, MemoryDeleteTool, MemoryListTool, MemorySearchTool, MemoryUpdateTool,
};
use crate::domain::types::{
    MemoryCategoryInput, MemoryLayerInput, MemoryLocationResult, ToolSchema,
};
use crate::domain::TypedTool;
use memory::api::reflection::ReflectionOutput;
use memory::api::{
    MemoryCategory, MemoryEntry, MemoryId, MemoryLayer, MemoryPolicy, MemoryPort, MemorySource,
    MemorySuggestion,
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

fn test_source() -> Arc<SwappableMemorySource> {
    Arc::new(SwappableMemorySource {
        current: RwLock::new(Arc::new(memory::api::NoOpMemory)),
    })
}

#[tokio::test]
async fn memory_search_tool_resolves_current_committed_port_for_each_call() {
    let first = Arc::new(
        memory::api::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap(),
    );
    first
        .write(test_entry("first committed memory", MemoryCategory::Fact))
        .await
        .unwrap();
    let second = Arc::new(
        memory::api::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap(),
    );
    second
        .write(test_entry("resumed committed memory", MemoryCategory::Fact))
        .await
        .unwrap();
    let source = Arc::new(SwappableMemorySource {
        current: RwLock::new(first),
    });
    let tool = MemorySearchTool {
        source: source.clone(),
    };
    let workspace = tempfile::tempdir().unwrap();
    let context = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build();

    let first_result = tool
        .call(serde_json::json!({"query": "committed memory"}), &context)
        .await;
    *source.current.write().unwrap() = second;
    let resumed_result = tool
        .call(serde_json::json!({"query": "committed memory"}), &context)
        .await;

    assert!(first_result.text.contains("first committed memory"));
    assert!(!first_result.text.contains("resumed committed memory"));
    assert!(resumed_result.text.contains("resumed committed memory"));
    assert!(!resumed_result.text.contains("first committed memory"));
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
    let memory =
        memory::api::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
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
    let memory =
        memory::api::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
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
    let memory =
        memory::api::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
    memory
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![MemorySuggestion {
                layer: MemoryLayer::Project,
                category: MemoryCategory::Preference,
                content: "Prefer deterministic lexical retrieval".to_string(),
                tags: vec!["reflection".to_string()],
                reason: "user preference".to_string(),
                supersedes: vec![],
                synthesizes: vec![],
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
    let memory =
        memory::api::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
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
    let memory = memory::api::InMemoryMemory::new_with_clock(
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
async fn memory_update_publishes_manageable_archive_and_restore_results() {
    let memory = Arc::new(
        memory::api::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap(),
    );
    let entry = test_entry("archive lifecycle", MemoryCategory::Decision);
    memory.write(entry.clone()).await.unwrap();
    let source = Arc::new(SwappableMemorySource {
        current: RwLock::new(memory.clone()),
    });
    let tool = MemoryUpdateTool { source };
    let workspace = tempfile::tempdir().unwrap();
    let context = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build();

    let archived = tool
        .call(
            serde_json::json!({"id": entry.id.to_string(), "status": "archive"}),
            &context,
        )
        .await;
    let restored = tool
        .call(
            serde_json::json!({"id": entry.id.to_string(), "status": "restore"}),
            &context,
        )
        .await;

    assert!(!archived.is_error);
    assert_eq!(archived.data.unwrap().action, "archive");
    assert!(!restored.is_error);
    assert_eq!(restored.data.unwrap().action, "restore");
    assert_eq!(memory.list(None), vec![entry]);
}

#[tokio::test]
async fn memory_update_pins_then_unpins() {
    let memory =
        memory::api::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
    let source = Arc::new(SwappableMemorySource {
        current: RwLock::new(Arc::new(memory)),
    });
    let workspace = tempfile::tempdir().unwrap();
    let context = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build();

    let add = MemoryAddTool {
        source: source.clone(),
    }
    .call(
        serde_json::json!({ "content": "偏好使用 tab 缩进" }),
        &context,
    )
    .await;
    assert!(!add.is_error, "add must succeed: {}", add.text);
    let id = add.data.unwrap().id.unwrap();

    let pinned = MemoryUpdateTool {
        source: source.clone(),
    }
    .call(serde_json::json!({ "id": id, "status": "pin" }), &context)
    .await;
    assert!(!pinned.is_error, "pin must succeed: {}", pinned.text);
    assert_eq!(pinned.data.unwrap().action, "pin");

    let unpinned = MemoryUpdateTool { source }
        .call(serde_json::json!({ "id": id, "status": "unpin" }), &context)
        .await;
    assert!(!unpinned.is_error, "unpin must succeed: {}", unpinned.text);
    assert_eq!(unpinned.data.unwrap().action, "unpin");
}

#[tokio::test]
async fn memory_update_archives_and_restores() {
    let memory =
        memory::api::InMemoryMemory::new_with_clock(MemoryPolicy::default(), || 2_000).unwrap();
    let source = Arc::new(SwappableMemorySource {
        current: RwLock::new(Arc::new(memory)),
    });
    let workspace = tempfile::tempdir().unwrap();
    let context = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build();

    let add = MemoryAddTool {
        source: source.clone(),
    }
    .call(
        serde_json::json!({ "content": "归档后仍可恢复的记忆" }),
        &context,
    )
    .await;
    assert!(!add.is_error, "add must succeed: {}", add.text);
    let id = add.data.unwrap().id.unwrap();

    let archived = MemoryUpdateTool {
        source: source.clone(),
    }
    .call(
        serde_json::json!({ "id": id, "status": "archive" }),
        &context,
    )
    .await;
    assert!(
        !archived.is_error,
        "archive must succeed: {}",
        archived.text
    );
    assert_eq!(archived.data.unwrap().action, "archive");

    let restored = MemoryUpdateTool { source }
        .call(
            serde_json::json!({ "id": id, "status": "restore" }),
            &context,
        )
        .await;
    assert!(
        !restored.is_error,
        "restore must succeed: {}",
        restored.text
    );
    assert_eq!(restored.data.unwrap().action, "restore");
}

#[tokio::test]
async fn memory_update_rejects_an_unknown_status() {
    let source = test_source();
    let workspace = tempfile::tempdir().unwrap();
    let context = crate::domain::test_support::TestToolExecutionContextBuilder::new(
        workspace.path().to_path_buf(),
    )
    .build();

    let explode = MemoryUpdateTool { source }
        .call(
            serde_json::json!({
                "id": "018f0000-0000-7000-8000-000000000000",
                "status": "explode"
            }),
            &context,
        )
        .await;
    assert!(
        explode.is_error,
        "unknown status must be rejected: {}",
        explode.text
    );
    assert!(explode.data.is_none());
}

#[test]
fn the_five_memory_tools_expose_type_driven_required() {
    let source = test_source();
    let add = MemoryAddTool {
        source: source.clone(),
    }
    .input_schema();
    assert_eq!(add["required"], serde_json::json!(["content"]));

    let search = MemorySearchTool {
        source: source.clone(),
    }
    .input_schema();
    assert_eq!(search["required"], serde_json::json!(["query"]));

    let list = MemoryListTool {
        source: source.clone(),
    }
    .input_schema();
    assert!(
        list.get("required").is_none(),
        "MemoryList has no required field"
    );

    let update = MemoryUpdateTool {
        source: source.clone(),
    }
    .input_schema();
    assert_eq!(update["required"], serde_json::json!(["id", "status"]));
    assert_eq!(
        update["properties"]["status"]["enum"],
        serde_json::json!(["pin", "unpin", "archive", "restore"])
    );

    let delete = MemoryDeleteTool {
        source: source.clone(),
    }
    .input_schema();
    assert_eq!(delete["required"], serde_json::json!(["id"]));
}

#[test]
fn no_memory_tool_schema_uses_combinator_keywords() {
    let source = test_source();
    for schema in [
        MemoryAddTool {
            source: source.clone(),
        }
        .input_schema(),
        MemorySearchTool {
            source: source.clone(),
        }
        .input_schema(),
        MemoryListTool {
            source: source.clone(),
        }
        .input_schema(),
        MemoryUpdateTool {
            source: source.clone(),
        }
        .input_schema(),
        MemoryDeleteTool {
            source: source.clone(),
        }
        .input_schema(),
    ] {
        let text = schema.to_string();
        for keyword in ["oneOf", "anyOf", "allOf"] {
            assert!(
                !text.contains(keyword),
                "schema must not use {keyword}: {text}"
            );
        }
    }
}

#[test]
fn memory_add_content_description_states_the_limit() {
    let schema = MemoryAddTool {
        source: test_source(),
    }
    .input_schema();
    let desc = schema["properties"]["content"]["description"]
        .as_str()
        .unwrap();
    assert!(
        desc.contains("500"),
        "description must state the 500-char limit: {desc}"
    );
}

#[test]
fn memory_add_schema_publishes_constrained_layers_and_categories() {
    let schema = MemoryAddTool {
        source: test_source(),
    }
    .input_schema();
    let properties = schema["properties"].as_object().unwrap();

    assert_eq!(
        properties["layer"]["enum"],
        serde_json::json!(["global", "project"])
    );
    assert_eq!(
        properties["category"]["enum"],
        serde_json::json!(["fact", "decision", "preference", "pattern", "pitfall"])
    );
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
