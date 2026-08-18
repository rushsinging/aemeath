use sdk::{
    ConnectCommand, ConnectErrorKind, ConnectOrigin, ConnectOutcome, ConnectRevision,
    ConnectSessionId, ConnectStage, ConnectView,
};

#[test]
fn connect_published_language_round_trips_without_credential_material() {
    let view = ConnectView {
        session_id: ConnectSessionId("session-1".to_string()),
        revision: ConnectRevision(3),
        stage: ConnectStage::Review,
        origin: ConnectOrigin::ExplicitCommand,
        catalog: Vec::new(),
        draft: sdk::ConnectDraftView {
            source: Some("Anthropic".to_string()),
            driver: Some("anthropic".to_string()),
            base_url: Some("https://example.test".to_string()),
            has_api_key: true,
            provider_user_agent: None,
            model: Some(sdk::ConnectModelDraftView {
                model_id: "model-1".to_string(),
                context_window: Some(32_000),
                max_tokens: Some(4_096),
            }),
            set_global_default: true,
        },
        existing_provider: None,
        available_actions: vec![sdk::ConnectAvailableAction::ConfirmSave],
        probe_status: Some(sdk::ConnectProbeStatus::Success { latency_ms: 4 }),
        last_error: None,
        terminal: None,
    };

    let encoded = serde_json::to_string(&view).unwrap();
    assert!(!encoded.contains("sk-secret"));
    assert!(!encoded.contains("credential"));
    assert_eq!(serde_json::from_str::<ConnectView>(&encoded).unwrap(), view);
}

#[test]
fn connect_view_publishes_catalog_options_without_credentials() {
    let catalog = sdk::ConnectProviderOption {
        source: "Anthropic".to_string(),
        driver: "anthropic".to_string(),
        default_base_url: "https://api.anthropic.com".to_string(),
        recommended_models: vec![sdk::ConnectRecommendedModelOption {
            model_id: "claude-sonnet-4-6".to_string(),
            context_window: 200_000,
            max_tokens: 64_000,
        }],
    };
    let encoded = serde_json::to_string(&catalog).unwrap();

    assert!(encoded.contains("Anthropic"));
    assert!(encoded.contains("claude-sonnet-4-6"));
    assert!(!encoded.to_ascii_lowercase().contains("api_key"));
}

#[test]
fn connect_commands_carry_typed_values_and_origins_are_distinct() {
    let command = ConnectCommand::SetCustomModel {
        model_id: "custom-model".to_string(),
        context_window: 64_000,
        max_tokens: 8_192,
    };
    let encoded = serde_json::to_value(command).unwrap();
    assert_eq!(encoded["set_custom_model"]["model_id"], "custom-model");
    assert_ne!(
        ConnectOrigin::ExplicitCommand,
        ConnectOrigin::FirstChatBootstrap
    );
    assert_ne!(
        ConnectOutcome::Cancelled,
        ConnectOutcome::Completed {
            applied_revision: 7
        }
    );
    assert_ne!(
        ConnectErrorKind::StaleRevision,
        ConnectErrorKind::ProbeFailed
    );
}
