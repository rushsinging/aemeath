use super::{
    tests::{LayerScript, ScriptedStore},
    MemoryService,
};
use crate::{
    CommittedMemoryDataset, MemoryCategory, MemoryCommitReceipt, MemoryCommitVisibility,
    MemoryDataset, MemoryEntry, MemoryError, MemoryId, MemoryLayer, MemoryPolicy, MemoryPort,
    MemorySource, MemorySuggestion, ReflectionOutput,
};

fn layer_script(
    loads: Vec<Result<CommittedMemoryDataset<u64>, MemoryError>>,
    commits: Vec<Result<MemoryCommitReceipt<u64>, MemoryError>>,
) -> LayerScript {
    LayerScript {
        loads: loads.into(),
        commits: commits.into(),
        ..LayerScript::default()
    }
}

fn empty_layer(revision: u64, layer: MemoryLayer) -> CommittedMemoryDataset<u64> {
    CommittedMemoryDataset {
        dataset: MemoryDataset::empty(layer),
        revision,
    }
}

fn committed(
    revision: u64,
    layer: MemoryLayer,
    entries: Vec<MemoryEntry>,
) -> CommittedMemoryDataset<u64> {
    CommittedMemoryDataset {
        dataset: MemoryDataset::new(layer, entries, vec![]).unwrap(),
        revision,
    }
}

fn receipt(revision: u64) -> MemoryCommitReceipt<u64> {
    MemoryCommitReceipt::new(revision, MemoryCommitVisibility::Visible)
}

fn storage_error() -> MemoryError {
    MemoryError::Storage {
        kind: crate::MemoryStorageErrorKind::Io,
    }
}

fn entry(layer: MemoryLayer, content: &str) -> MemoryEntry {
    MemoryEntry::new(
        MemoryId::now_v7(),
        100,
        layer,
        MemoryCategory::Fact,
        content,
        MemorySource::User,
    )
    .unwrap()
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
async fn reflection_partial_apply_reports_committed_suggestion_before_outdated_write_failure() {
    let existing = entry(MemoryLayer::Project, "obsolete fact");
    let existing_id = existing.id;
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Project, vec![existing]))],
            vec![Ok(receipt(2)), Err(storage_error())],
        ),
    );
    let service = MemoryService::open_with_clock(store, MemoryPolicy::default(), || 200)
        .await
        .unwrap();

    let error = service
        .apply_reflection(&ReflectionOutput {
            suggested_memories: vec![suggestion(MemoryLayer::Project, "current fact")],
            outdated_memories: vec![existing_id.to_string()],
            ..ReflectionOutput::default()
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        MemoryError::PartialApply {
            result_attempted: 2,
            result_completed: 1,
            suggestions_added: 1,
            outdated_marked: 0,
        }
    ));
    let entries = service.list(Some(MemoryLayer::Project));
    assert!(entries.iter().any(|entry| entry.content == "current fact"));
    assert!(entries
        .iter()
        .any(|entry| entry.id == existing_id && !entry.outdated));
}

#[tokio::test]
async fn retrieve_for_inject_reads_committed_memory_without_write() {
    let stored = entry(MemoryLayer::Project, "read only fact");
    let store = ScriptedStore::new(
        layer_script(vec![Ok(empty_layer(1, MemoryLayer::Global))], vec![]),
        layer_script(
            vec![Ok(committed(1, MemoryLayer::Project, vec![stored.clone()]))],
            vec![],
        ),
    );
    let observer = store.clone();
    let service = MemoryService::open(store, MemoryPolicy::default())
        .await
        .unwrap();

    let result = service.retrieve_for_inject(&crate::MemoryQuery {
        limit: 1,
        layer: Some(MemoryLayer::Project),
        category: None,
        now: 200,
    });

    assert_eq!(result.mode, crate::MemoryRetrievalMode::InjectionPriority);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].entry, stored);
    assert_eq!(
        observer.calls(MemoryLayer::Project),
        (1, 0),
        "injection retrieval must not write the committed project layer"
    );
}
