use std::fs;

use super::*;
use crate::catalog::find_by_source;
use crate::connect::{ConnectDraft, ModelDraft};

fn draft() -> ConnectDraft {
    let entry = find_by_source("Anthropic").unwrap();
    let mut draft = ConnectDraft::empty();
    draft.source = Some(entry.source);
    draft.driver = Some(entry.driver);
    draft.base_url = Some("https://example.test".to_string());
    draft.provider_user_agent = Some("test-agent".to_string());
    draft.model = Some(ModelDraft {
        model_id: "model-x".to_string(),
        context_window: 32_000,
        max_tokens: 4_096,
    });
    draft.set_global_default = true;
    draft.set_user_credential("secret".to_string());
    draft
}

#[test]
fn create_default_is_create_new_and_does_not_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let store = FilesystemGlobalConfigConnectStore::new(dir.path().to_path_buf());
    let first = store.create_complete_default().unwrap();
    assert!(store.config_path().is_file());
    assert!(first.digest.as_str().len() >= 32);
    let original = fs::read(store.config_path()).unwrap();
    let second = store.create_complete_default().unwrap_err();
    assert!(matches!(second, GlobalConfigStoreError::AlreadyExists));
    assert_eq!(fs::read(store.config_path()).unwrap(), original);
}

#[test]
fn commit_replaces_only_target_provider_and_preserves_other_fields() {
    let dir = tempfile::tempdir().unwrap();
    let store = FilesystemGlobalConfigConnectStore::new(dir.path().to_path_buf());
    fs::create_dir_all(dir.path()).unwrap();
    fs::write(
        store.config_path(),
        br#"{"language":"zh","models":{"default":"Other/old","providers":{"Other":{"baseUrl":"https://other.test","apiKey":"keep","driver":"openai","models":[]}}},"future":{"keep":true}}"#,
    )
    .unwrap();
    let loaded = store.load_global_document().unwrap().unwrap();
    let original_revision = loaded.revision.clone();
    let receipt = store
        .commit_draft(original_revision.clone(), &draft())
        .expect("commit succeeds");
    assert_ne!(receipt.revision, original_revision);
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(store.config_path()).unwrap()).unwrap();
    assert_eq!(value["language"], "zh");
    assert_eq!(value["future"]["keep"], true);
    assert_eq!(value["models"]["providers"]["Other"]["apiKey"], "keep");
    assert_eq!(
        value["models"]["providers"]["Anthropic"]["apiKey"],
        "secret"
    );
    assert_eq!(value["models"]["default"], "Anthropic/model-x");
}

#[test]
fn stale_revision_conflicts_without_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let store = FilesystemGlobalConfigConnectStore::new(dir.path().to_path_buf());
    store.create_complete_default().unwrap();
    let loaded = store.load_global_document().unwrap().unwrap();
    let external = b"{\"language\":\"external\"}";
    fs::write(store.config_path(), external).unwrap();
    let error = store.commit_draft(loaded.revision, &draft()).unwrap_err();
    assert!(matches!(error, GlobalConfigStoreError::Conflict { .. }));
    assert_eq!(fs::read(store.config_path()).unwrap(), external);
}

#[test]
fn rollback_requires_matching_bootstrap_digest() {
    let dir = tempfile::tempdir().unwrap();
    let store = FilesystemGlobalConfigConnectStore::new(dir.path().to_path_buf());
    let receipt = store.create_complete_default().unwrap();
    fs::write(store.config_path(), b"{\"language\":\"changed\"}").unwrap();
    let error = store.rollback_bootstrap(&receipt).unwrap_err();
    assert!(matches!(error, GlobalConfigStoreError::RollbackRefused));
    assert!(store.config_path().exists());
}

#[test]
fn rollback_matching_receipt_removes_only_bootstrap_document() {
    let dir = tempfile::tempdir().unwrap();
    let store = FilesystemGlobalConfigConnectStore::new(dir.path().to_path_buf());
    let receipt = store.create_complete_default().unwrap();
    store.rollback_bootstrap(&receipt).unwrap();
    assert!(!store.config_path().exists());
}
