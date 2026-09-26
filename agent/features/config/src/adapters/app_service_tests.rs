//! ConfigAppService（adapters 层 port 实现）测试。
use super::*;
use crate::adapters::ConfigAppService;
use crate::domain::*;
use crate::ports::{ConfigReader, ConfigWriter, ProjectConfigParticipant};
use share::config::domain::merge::ConfigPatch;

struct FakeEnv(std::collections::HashMap<String, String>);

impl EnvSource for FakeEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

#[tokio::test]
async fn cli_layer_overrides_env() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("config.json");
    let service = ConfigAppService::with_global_path(Some(dir.path()), global).with_env_source(
        std::sync::Arc::new(FakeEnv(std::collections::HashMap::from([(
            "AEMEATH_MODEL".into(),
            "env-model".into(),
        )]))),
    );
    service
        .set_cli_patch(crate::adapters::CliArgsAdapter::read(
            &crate::adapters::CliConfigInputData {
                api_key: Some("cli-key".into()),
                model: Some("cli-model".into()),
                ..Default::default()
            },
        ))
        .await;
    service.load().await.unwrap();
    assert_eq!(service.committed_snapshot().model_name(), "cli-model");
    assert_eq!(service.committed_snapshot().api_key(), Some("cli-key"));
}

#[tokio::test]
async fn update_replaces_committed_snapshot_even_without_receiver() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let service =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
            .with_native_store(NativeConfigStore::new(storage));
    service
        .update(ConfigUpdateData::SetModel {
            model: "provider/model".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        service.committed_snapshot().models().default,
        "provider/model"
    );
}

#[tokio::test]
async fn consecutive_updates_preserve_previously_committed_fields() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let service =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
            .with_native_store(NativeConfigStore::new(storage));

    service
        .update(ConfigUpdateData::SetModel {
            model: "provider/model".into(),
        })
        .await
        .unwrap();
    service
        .update(ConfigUpdateData::SetPermissionMode {
            mode: share::config::PermissionModeConfig::AllowAll,
        })
        .await
        .unwrap();

    let snapshot = service.committed_snapshot();
    assert_eq!(snapshot.models().default, "provider/model");
    assert_eq!(snapshot.model_name(), "provider/model");
    assert_eq!(
        snapshot.permission_mode(),
        share::config::PermissionModeConfig::AllowAll
    );
    let rebuilt =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
            .with_native_store(NativeConfigStore::new(
                storage::file_system_blob(dir.path()).unwrap(),
            ));
    rebuilt.load().await.unwrap();
    let snapshot = rebuilt.committed_snapshot();
    assert_eq!(snapshot.models().default, "provider/model");
    assert_eq!(
        snapshot.permission_mode(),
        share::config::PermissionModeConfig::AllowAll
    );
}

#[tokio::test]
async fn concurrent_updates_are_serialized_without_losing_fields() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let service = std::sync::Arc::new(
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
            .with_native_store(NativeConfigStore::new(storage)),
    );
    let model = {
        let service = service.clone();
        tokio::spawn(async move {
            service
                .update(ConfigUpdateData::SetModel {
                    model: "concurrent/model".into(),
                })
                .await
        })
    };
    let permission = {
        let service = service.clone();
        tokio::spawn(async move {
            service
                .update(ConfigUpdateData::SetPermissionMode {
                    mode: share::config::PermissionModeConfig::AllowAll,
                })
                .await
        })
    };
    model.await.unwrap().unwrap();
    permission.await.unwrap().unwrap();

    let snapshot = service.committed_snapshot();
    assert_eq!(snapshot.models().default, "concurrent/model");
    assert_eq!(
        snapshot.permission_mode(),
        share::config::PermissionModeConfig::AllowAll
    );
}

#[tokio::test]
async fn runtime_override_is_restored_after_service_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("config.json");
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let store = NativeConfigStore::new(storage);
    let service =
        ConfigAppService::with_global_path(None, global.clone()).with_native_store(store.clone());
    service
        .update(ConfigUpdateData::SetModel {
            model: "runtime/model".into(),
        })
        .await
        .unwrap();
    drop(service);

    let rebuilt = ConfigAppService::with_global_path(None, global).with_native_store(store);
    rebuilt.load().await.unwrap();

    assert_eq!(
        rebuilt.committed_snapshot().models().default,
        "runtime/model"
    );
}

#[tokio::test]
async fn prepare_update_does_not_publish_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let service =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
            .with_native_store(NativeConfigStore::new(storage));
    let before = service.committed_snapshot().models().default.clone();
    let prepared = service
        .prepare_update(ConfigUpdateData::SetModel {
            model: "local/model".into(),
        })
        .await
        .unwrap();
    assert_eq!(service.committed_snapshot().models().default, before);
    let ready = service
        .persist_update(prepared)
        .await
        .unwrap_or_else(|error| panic!("unexpected {error:?}"));
    service.commit_update(*ready);
    assert_eq!(service.committed_snapshot().models().default, "local/model");
}

#[tokio::test]
async fn env_permission_override_remains_above_dynamic_local_update() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let service =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
            .with_native_store(NativeConfigStore::new(storage))
            .with_env_source(std::sync::Arc::new(FakeEnv(
                std::collections::HashMap::from([(
                    "AEMEATH_PERMISSION_MODE".into(),
                    "allow_all".into(),
                )]),
            )));
    service.load().await.unwrap();

    service
        .update(ConfigUpdateData::SetPermissionMode {
            mode: share::config::PermissionModeConfig::Ask,
        })
        .await
        .unwrap();

    assert_eq!(
        service.committed_snapshot().permission_mode(),
        share::config::PermissionModeConfig::AllowAll
    );
}

#[tokio::test]
async fn cli_permission_override_remains_highest_after_dynamic_update() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let service =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
            .with_native_store(NativeConfigStore::new(storage));
    service
        .set_cli_patch(crate::adapters::CliArgsAdapter::read(
            &crate::adapters::CliConfigInputData {
                allow_all: true,
                ..Default::default()
            },
        ))
        .await;
    service.load().await.unwrap();

    service
        .update(ConfigUpdateData::SetPermissionMode {
            mode: share::config::PermissionModeConfig::Ask,
        })
        .await
        .unwrap();

    assert_eq!(
        service.committed_snapshot().permission_mode(),
        share::config::PermissionModeConfig::AllowAll
    );
}

#[tokio::test]
async fn complete_priority_contract_uses_cli_over_env_over_local_over_global() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global.json");
    std::fs::write(&global, r#"{"model":{"name":"global"}}"#).unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".agents")).unwrap();
    std::fs::write(
        project.join(".agents/aemeath.json"),
        r#"{"model":{"name":"project"}}"#,
    )
    .unwrap();
    let storage = storage::file_system_blob(dir.path().join("storage")).unwrap();
    let store = NativeConfigStore::new(storage);
    let runtime = ConfigPatch {
        model: Some(share::config::domain::merge::ModelConfigPatch {
            name: Some("runtime".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    store
        .write_override("global", &encode_native_patch(&runtime).unwrap())
        .await
        .unwrap();
    let service = ConfigAppService::with_global_path(Some(&project), global)
        .with_native_store(store)
        .with_env_source(std::sync::Arc::new(FakeEnv(
            std::collections::HashMap::from([("AEMEATH_MODEL".into(), "env".into())]),
        )));
    service
        .set_cli_patch(crate::adapters::CliArgsAdapter::read(
            &crate::adapters::CliConfigInputData {
                model: Some("cli".into()),
                ..Default::default()
            },
        ))
        .await;

    service.load().await.unwrap();

    assert_eq!(service.committed_snapshot().model_name(), "cli");
}

#[tokio::test]
async fn persist_failure_does_not_publish_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let service =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"));
    let before = service.committed_snapshot().models().default.clone();

    let error = service
        .update(ConfigUpdateData::SetModel {
            model: "uncommitted/model".into(),
        })
        .await
        .unwrap_err();

    assert_eq!(
        error,
        share::error::DomainError::from(ConfigPersistError::UnsupportedDurability)
    );
    assert_eq!(
        error.category(),
        share::error::ErrorCategory::Invalid,
        "UnsupportedDurability 折叠为 invalid"
    );
    assert_eq!(service.committed_snapshot().models().default, before);
}

#[tokio::test]
async fn committed_update_notifies_subscription_with_same_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let storage = storage::file_system_blob(dir.path()).unwrap();
    let service =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
            .with_native_store(NativeConfigStore::new(storage));
    let mut subscription = ConfigReader::subscribe(&service).await.unwrap();

    service
        .update(ConfigUpdateData::SetModel {
            model: "notified/model".into(),
        })
        .await
        .unwrap();
    subscription.changes.changed().await.unwrap();

    assert_eq!(
        subscription.changes.borrow().models().default,
        "notified/model"
    );
    assert_eq!(
        subscription.changes.borrow().models().default,
        service.committed_snapshot().models().default
    );
}

#[tokio::test]
async fn project_commit_becomes_baseline_for_following_update() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join(".agents")).unwrap();
    std::fs::write(
        project.join(".agents/aemeath.json"),
        r#"{"model":{"name":"project-model"}}"#,
    )
    .unwrap();
    let root = project.canonicalize().unwrap();
    let location =
        ProjectConfigLocationData::try_from_project_identity(root, b"project-a").unwrap();
    let storage = storage::file_system_blob(dir.path().join("storage")).unwrap();
    let service = ConfigAppService::with_global_path(None, dir.path().join("global.json"))
        .with_native_store(NativeConfigStore::new(storage));

    let prepared = service.prepare_for_project(&location).await.unwrap();
    service.commit_project(prepared).await;
    service
        .update(ConfigUpdateData::SetPermissionMode {
            mode: share::config::PermissionModeConfig::AllowAll,
        })
        .await
        .unwrap();

    let snapshot = service.committed_snapshot();
    assert_eq!(snapshot.model_name(), "project-model");
    assert_eq!(
        snapshot.permission_mode(),
        share::config::PermissionModeConfig::AllowAll
    );
}

#[tokio::test]
async fn subscription_initial_matches_committed_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let service =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"));
    let subscription = ConfigReader::subscribe(&service).await.unwrap();
    assert_eq!(
        subscription.initial.model_name(),
        service.committed_snapshot().model_name()
    );
}

// ─────────────────────────────────────────────────────────────────
// Logging contract for the real assembly entry
// `wire_project_config_with_cli`.
//
#[tokio::test]
async fn project_aemeath_overrides_claude_compatibility() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agents")).unwrap();
    std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
    std::fs::write(
        dir.path().join(".agents/aemeath.json"),
        r#"{"model":{"name":"aemeath"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".claude/settings.json"),
        r#"{"model":"claude"}"#,
    )
    .unwrap();
    let service =
        ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("global.json"));
    service.load().await.unwrap();

    assert_eq!(service.committed_snapshot().model_name(), "aemeath");
}

#[tokio::test]
async fn refresh_rejects_invalid_source_and_preserves_committed_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global.json");
    std::fs::write(&global, r#"{"model":{"name":"first"}}"#).unwrap();
    let service = ConfigAppService::with_global_path(None, global.clone());
    service.load().await.unwrap();
    let before = service.committed_snapshot();

    std::fs::write(&global, "not json").unwrap();
    assert!(matches!(
        service.refresh_if_sources_changed().await,
        Err(error)
            if error.category() == share::error::ErrorCategory::Invalid
                && error.message() == "配置源解析失败"
    ));
    assert_eq!(service.committed_snapshot().model_name(), "first");
    assert_eq!(service.committed_snapshot().revision(), before.revision());
}

#[tokio::test]
async fn refresh_does_not_publish_file_change_overridden_by_env() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global.json");
    std::fs::write(&global, r#"{"model":{"name":"first"}}"#).unwrap();
    let service = ConfigAppService::with_global_path(None, global.clone()).with_env_source(
        std::sync::Arc::new(FakeEnv(std::collections::HashMap::from([(
            "AEMEATH_MODEL".into(),
            "env-model".into(),
        )]))),
    );
    service.load().await.unwrap();
    let before = service.committed_snapshot();

    std::fs::write(&global, r#"{"model":{"name":"second"}}"#).unwrap();
    assert!(matches!(
        service.refresh_if_sources_changed().await,
        Ok(ConfigRefreshOutcomeData::Unchanged)
    ));
    assert_eq!(service.committed_snapshot().model_name(), "env-model");
    assert_eq!(service.committed_snapshot().revision(), before.revision());
}

#[tokio::test]
async fn refresh_reports_run_scope_for_allow_all() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global.json");
    std::fs::write(&global, r#"{"permissions":{"mode":"ask"}}"#).unwrap();
    let service = ConfigAppService::with_global_path(None, global.clone());
    service.load().await.unwrap();

    std::fs::write(&global, r#"{"permissions":{"mode":"allow_all"}}"#).unwrap();
    let outcome = service.refresh_if_sources_changed().await;

    assert!(matches!(
        outcome,
        Ok(ConfigRefreshOutcomeData::Reloaded { scopes, .. })
            if scopes == vec![share::config::domain::scope::ConfigApplicationScope::Run]
    ));
}

#[tokio::test]
async fn refresh_reports_session_restart_scope_for_tui_change() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global.json");
    std::fs::write(&global, r#"{"ui":{"tui":true}}"#).unwrap();
    let service = ConfigAppService::with_global_path(None, global.clone());
    service.load().await.unwrap();

    std::fs::write(&global, r#"{"ui":{"tui":false}}"#).unwrap();
    let outcome = service.refresh_if_sources_changed().await;

    assert!(matches!(
        outcome,
        Ok(ConfigRefreshOutcomeData::Reloaded { scopes, .. })
            if scopes == vec![share::config::domain::scope::ConfigApplicationScope::SessionRestartRequired]
    ));
}

#[tokio::test]
async fn refresh_publishes_to_watch_subscribers_once() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global.json");
    std::fs::write(&global, r#"{"model":{"name":"first"}}"#).unwrap();
    let service = ConfigAppService::with_global_path(None, global.clone());
    service.load().await.unwrap();
    let mut changes = service.subscribe_committed();

    std::fs::write(&global, r#"{"model":{"name":"second"}}"#).unwrap();
    assert!(matches!(
        service.refresh_if_sources_changed().await,
        Ok(ConfigRefreshOutcomeData::Reloaded { .. })
    ));
    changes.changed().await.unwrap();
    assert_eq!(changes.borrow().model_name(), "second");
}
