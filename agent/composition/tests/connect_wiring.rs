use composition::app::{
    build_connect_bootstrap_with_agents_dir, ConnectFacade, GlobalConnectCommitAdapter,
};
use config::connect::{ConnectAppService, ConnectOrigin};
use std::sync::Arc;

#[tokio::test]
async fn connect_bootstrap_exposes_provider_connect_as_generic_form_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let bootstrap = build_connect_bootstrap_with_agents_dir(temp.path())
        .await
        .unwrap();

    let view = bootstrap
        .forms
        .start_form(
            sdk::ConfigFormWorkflowId("provider_connect".to_string()),
            sdk::ConfigFormOrigin::ExplicitCommand,
        )
        .await
        .unwrap();

    assert_eq!(view.workflow_id.as_str(), "provider_connect");
    assert_eq!(view.page.id.as_str(), "select_provider");
    assert!(view
        .page
        .fields
        .iter()
        .any(|field| field.id.as_str() == "provider_source"));
}

#[tokio::test]
async fn first_chat_rollback_refuses_to_delete_externally_modified_default() {
    let temp = tempfile::tempdir().unwrap();
    let bootstrap = composition::app::prepare_first_chat_with_agents_dir(temp.path(), true)
        .await
        .unwrap()
        .unwrap();
    std::fs::write(
        temp.path().join("aemeath.json"),
        r#"{"language":"external-change"}"#,
    )
    .unwrap();

    let error = bootstrap.rollback().await.unwrap_err();

    assert!(error.to_string().contains("拒绝回滚"));
    assert!(temp.path().join("aemeath.json").exists());
}

#[tokio::test]
async fn first_chat_preflight_creates_full_default_and_can_rollback_unchanged_receipt() {
    let temp = tempfile::tempdir().unwrap();

    let bootstrap = composition::app::prepare_first_chat_with_agents_dir(temp.path(), true)
        .await
        .unwrap()
        .expect("missing config must enter connect");
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(temp.path().join("aemeath.json")).unwrap()).unwrap();

    assert!(value.get("models").is_some());
    let view = bootstrap
        .connect
        .start_connect(sdk::ConnectOrigin::FirstChatBootstrap)
        .await
        .unwrap();
    assert_eq!(view.origin, sdk::ConnectOrigin::FirstChatBootstrap);
    bootstrap.rollback().await.unwrap();
    assert!(!temp.path().join("aemeath.json").exists());
}

#[tokio::test]
async fn first_chat_preflight_rejects_non_interactive_missing_config_without_creating_it() {
    let temp = tempfile::tempdir().unwrap();

    let error = match composition::app::prepare_first_chat_with_agents_dir(temp.path(), false).await
    {
        Ok(_) => panic!("non-interactive bootstrap must fail"),
        Err(error) => error,
    };

    assert!(matches!(error, sdk::SdkError::Init(message) if message.contains("aemeath connect")));
    assert!(!temp.path().join("aemeath.json").exists());
}

#[tokio::test]
async fn selecting_an_existing_provider_requires_overwrite_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("aemeath.json"),
        r#"{"models":{"providers":{"Anthropic":{"driver":"anthropic","baseUrl":"https://existing.test","apiKey":"existing-secret","models":[{"id":"existing-model","contextWindow":1000,"maxTokens":100}]}}}}"#,
    )
    .unwrap();
    let bootstrap = build_connect_bootstrap_with_agents_dir(temp.path())
        .await
        .unwrap();
    let start = bootstrap
        .connect
        .start_connect(sdk::ConnectOrigin::ExplicitCommand)
        .await
        .unwrap();
    let selected = bootstrap
        .connect
        .apply_connect(
            start.session_id,
            start.revision,
            sdk::ConnectCommand::SelectProvider {
                source: "Anthropic".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(selected.stage, sdk::ConnectStage::ConfirmOverwrite);
    let existing = selected.existing_provider.as_ref().unwrap();
    assert_eq!(existing.base_url, "https://existing.test");
    assert!(existing.has_api_key);
    assert!(!serde_json::to_string(&selected)
        .unwrap()
        .contains("existing-secret"));
}

#[tokio::test]
async fn connect_bootstrap_requires_only_global_connect_infrastructure() {
    let temp = tempfile::tempdir().unwrap();

    let bootstrap = build_connect_bootstrap_with_agents_dir(temp.path())
        .await
        .unwrap();
    let view = bootstrap
        .connect
        .start_connect(sdk::ConnectOrigin::ExplicitCommand)
        .await
        .unwrap();

    assert_eq!(view.stage, sdk::ConnectStage::SelectProvider);
    assert!(temp.path().join("aemeath.json").exists());
    assert!(!temp.path().join("sessions").exists());
}

#[tokio::test]
async fn connect_facade_maps_identity_revision_origin_and_redacted_view() {
    let temp = tempfile::tempdir().unwrap();
    let store: Arc<dyn config::GlobalConfigConnectStore> = Arc::new(
        config::FilesystemGlobalConfigConnectStore::new(temp.path().to_path_buf()),
    );
    store.create_complete_default().await.unwrap();
    let commit = GlobalConnectCommitAdapter::new(store.clone());
    let service = Arc::new(
        ConnectAppService::builder()
            .with_catalog(config::catalog::PROVIDER_CATALOG)
            .with_probe(composition::provider::ProviderProbeAdapter::new())
            .with_commit(commit)
            .build(),
    );
    let facade = ConnectFacade::new(service, store);

    let view = facade
        .start(sdk::ConnectOrigin::ExplicitCommand, None)
        .await
        .unwrap();
    assert_eq!(view.origin, sdk::ConnectOrigin::ExplicitCommand);
    assert_eq!(view.stage, sdk::ConnectStage::SelectProvider);
    assert_eq!(view.revision, sdk::ConnectRevision(0));
    assert!(!view.session_id.0.is_empty());

    let edited = facade
        .apply(
            view.session_id,
            view.revision,
            sdk::ConnectCommand::SelectProvider {
                source: "Anthropic".to_string(),
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.stage, sdk::ConnectStage::EditEndpoint);
    assert_eq!(edited.revision, sdk::ConnectRevision(1));
    assert!(!serde_json::to_string(&edited)
        .unwrap()
        .contains("secret-key"));
}

#[tokio::test]
async fn commit_adapter_uses_session_start_revision_instead_of_reloading_latest() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(config::FilesystemGlobalConfigConnectStore::new(
        temp.path().to_path_buf(),
    ));
    store.create_complete_default().unwrap();
    let started = store.load_global_document().unwrap().unwrap();
    std::fs::write(store.config_path(), r#"{"language":"external"}"#).unwrap();
    let mut draft = config::connect::ConnectDraft::empty();
    let entry = config::catalog::find_by_source("Anthropic").unwrap();
    draft.source = Some(entry.source);
    draft.driver = Some(entry.driver);
    draft.base_url = Some("https://example.test".to_string());
    draft.model = Some(config::connect::ModelDraft {
        model_id: "model-1".to_string(),
        context_window: 32_000,
        max_tokens: 4_096,
    });
    let adapter = GlobalConnectCommitAdapter::new(store.clone());
    let error = config::connect::ConnectCommitPort::commit(
        adapter.as_ref(),
        config::connect::ConnectCommitRequest {
            session_id: config::connect::ConnectSessionId::from_transport_str(
                "018f6f7a-7c2b-7000-8000-000000000001",
            )
            .unwrap(),
            origin: ConnectOrigin::ExplicitCommand,
            expected_global_revision: started.revision,
            draft,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        config::connect::ConnectCommitError::PersistConflict { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(store.config_path()).unwrap(),
        r#"{"language":"external"}"#
    );
}

#[tokio::test]
async fn config_origin_remains_distinct_through_composition_acl() {
    let temp = tempfile::tempdir().unwrap();
    let store: Arc<dyn config::GlobalConfigConnectStore> = Arc::new(
        config::FilesystemGlobalConfigConnectStore::new(temp.path().to_path_buf()),
    );
    store.create_complete_default().await.unwrap();
    let service = Arc::new(
        ConnectAppService::builder()
            .with_catalog(config::catalog::PROVIDER_CATALOG)
            .with_probe(composition::provider::ProviderProbeAdapter::new())
            .without_commit()
            .build(),
    );
    let facade = ConnectFacade::new(service, store);
    let first_chat = facade
        .start(sdk::ConnectOrigin::FirstChatBootstrap, None)
        .await
        .unwrap();
    assert_eq!(first_chat.origin, sdk::ConnectOrigin::FirstChatBootstrap);
    assert_ne!(first_chat.origin, sdk::ConnectOrigin::ExplicitCommand);
    let _ = ConnectOrigin::FirstChatBootstrap;
}
