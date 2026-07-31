use std::sync::Arc;

#[test]
fn connect_reducer_masks_credential_and_submits_typed_effect() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let view = sdk::ConnectView {
        session_id: sdk::ConnectSessionId("connect-1".to_string()),
        revision: sdk::ConnectRevision(3),
        stage: sdk::ConnectStage::EditCredential,
        origin: sdk::ConnectOrigin::ExplicitCommand,
        catalog: Vec::new(),
        draft: sdk::ConnectDraftView {
            source: Some("Anthropic".to_string()),
            driver: Some("anthropic".to_string()),
            base_url: Some("https://api.anthropic.com".to_string()),
            has_api_key: false,
            provider_user_agent: None,
            model: None,
            set_global_default: false,
        },
        existing_provider: None,
        available_actions: vec![sdk::ConnectAvailableAction::SetCredential],
        probe_status: None,
        last_error: None,
        terminal: None,
    };
    let mut model = crate::subcommand::connect_command::ConnectUiModel::new(view);
    model.update(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));

    assert_eq!(model.visible_input(), "•");
    assert!(!model.visible_input().contains('s'));
    assert!(matches!(
        model.update(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(crate::subcommand::connect_command::ConnectEffect::Apply {
            revision: sdk::ConnectRevision(3),
            command: sdk::ConnectCommand::SetCredential { api_key },
            ..
        }) if api_key == "s"
    ));
}

#[test]
fn connect_effect_carries_session_revision_and_typed_command() {
    let effect = crate::subcommand::connect_command::ConnectEffect::Apply {
        session_id: sdk::ConnectSessionId("connect-1".to_string()),
        revision: sdk::ConnectRevision(8),
        command: sdk::ConnectCommand::SkipProbe,
    };

    assert!(matches!(
        effect,
        crate::subcommand::connect_command::ConnectEffect::Apply {
            revision: sdk::ConnectRevision(8),
            command: sdk::ConnectCommand::SkipProbe,
            ..
        }
    ));
}

#[test]
fn credential_stage_uses_masked_input_mode() {
    assert_eq!(
        crate::subcommand::connect_command::input_mode_for_stage(sdk::ConnectStage::EditCredential),
        crate::subcommand::connect_command::ConnectInputMode::Masked
    );
    assert_eq!(
        crate::subcommand::connect_command::input_mode_for_stage(sdk::ConnectStage::EditEndpoint),
        crate::subcommand::connect_command::ConnectInputMode::Visible
    );
}

#[test]
fn connect_projection_lists_server_published_catalog() {
    let view = sdk::ConnectView {
        session_id: sdk::ConnectSessionId("connect-1".to_string()),
        revision: sdk::ConnectRevision(0),
        stage: sdk::ConnectStage::SelectProvider,
        origin: sdk::ConnectOrigin::ExplicitCommand,
        catalog: vec![sdk::ConnectProviderOption {
            source: "Anthropic".to_string(),
            driver: "anthropic".to_string(),
            default_base_url: String::new(),
            recommended_models: Vec::new(),
        }],
        draft: sdk::ConnectDraftView {
            source: None,
            driver: None,
            base_url: None,
            has_api_key: false,
            provider_user_agent: None,
            model: None,
            set_global_default: false,
        },
        existing_provider: None,
        available_actions: vec![sdk::ConnectAvailableAction::SelectProvider],
        probe_status: None,
        last_error: None,
        terminal: None,
    };

    let projection = crate::subcommand::connect_command::ConnectProjection::from_view(&view);
    assert!(projection
        .lines()
        .iter()
        .any(|line| line == "  Anthropic (anthropic)"));
}

#[test]
fn connect_projection_masks_api_key_and_emits_only_typed_sdk_commands() {
    let view = sdk::ConnectView {
        session_id: sdk::ConnectSessionId("connect-1".to_string()),
        revision: sdk::ConnectRevision(4),
        stage: sdk::ConnectStage::EditCredential,
        origin: sdk::ConnectOrigin::ExplicitCommand,
        catalog: Vec::new(),
        draft: sdk::ConnectDraftView {
            source: Some("Anthropic".to_string()),
            driver: Some("anthropic".to_string()),
            base_url: Some("https://api.anthropic.com".to_string()),
            has_api_key: true,
            provider_user_agent: None,
            model: None,
            set_global_default: false,
        },
        existing_provider: None,
        available_actions: vec![sdk::ConnectAvailableAction::SetCredential],
        probe_status: None,
        last_error: None,
        terminal: None,
    };
    let projection = crate::subcommand::connect_command::ConnectProjection::from_view(&view);

    assert!(projection
        .lines()
        .iter()
        .any(|line| line.contains("••••••••")));
    assert!(!projection
        .lines()
        .iter()
        .any(|line| line.contains("secret-key")));
    assert_eq!(
        crate::subcommand::connect_command::command_for_input(&view, "secret-key").unwrap(),
        sdk::ConnectCommand::SetCredential {
            api_key: "secret-key".to_string()
        }
    );
}

#[test]
fn tui_and_no_tui_share_the_same_router_contract() {
    let wiring = composition::tools::wire_commands().expect("command wiring");
    let catalog = wiring.catalog();
    let router = wiring.router();

    let tui = crate::tui::app::slash::resolve_slash_for_delivery(router.as_ref(), "/quit")
        .expect("TUI route");
    let no_tui = crate::chat::no_tui::resolve_slash_for_delivery(router.as_ref(), "/quit")
        .expect("no-TUI route");
    assert_eq!(tui, no_tui);
    assert!(catalog
        .complete("/qu")
        .iter()
        .any(|completion| completion.replacement == "/quit"));
}

#[test]
fn unknown_slash_is_rejected_before_it_can_become_user_input() {
    let wiring = composition::tools::wire_commands().expect("command wiring");
    let error = crate::chat::no_tui::resolve_slash_for_delivery(
        wiring.router().as_ref(),
        "/unknown-command",
    )
    .expect_err("unknown command must fail closed");

    assert!(matches!(
        error,
        sdk::CommandParseError::UnknownCommand { .. }
    ));
}

#[test]
fn help_and_completion_come_from_the_injected_catalog() {
    let wiring = composition::tools::wire_commands().expect("command wiring");
    let help = crate::tui::app::slash::help::command_help_lines(wiring.catalog().as_ref());

    assert!(help.iter().any(|line| line.contains("/help")));
    assert!(help.iter().any(|line| line.contains("/quit")));
    assert!(!help.iter().any(|line| line.contains("/think")));
}

#[test]
fn tui_and_no_tui_preserve_prompt_injection_and_invalid_name_results() {
    let skills = std::collections::HashMap::from([(
        "review".to_string(),
        sdk::SkillView {
            name: "review".to_string(),
            aliases: Vec::new(),
            slash_command: Some("review".to_string()),
            slash_aliases: vec!["cr".to_string()],
            description: "Review".to_string(),
            argument_hint: None,
        },
    )]);
    let wiring = composition::tools::wire_commands_with_skills(&skills).unwrap();
    let router = wiring.router();

    let tui = crate::tui::app::slash::resolve_slash_for_delivery(router.as_ref(), "/cr staged");
    let no_tui = crate::chat::no_tui::resolve_slash_for_delivery(router.as_ref(), "/cr staged");
    assert_eq!(tui, no_tui);
    assert!(matches!(
        tui,
        Ok(sdk::CommandRoute::SkillRequest(command))
            if command.command.as_str() == "review" && command.arguments.as_slice() == ["staged"]
    ));

    let tui_invalid =
        crate::tui::app::slash::resolve_slash_for_delivery(router.as_ref(), "/bad::name");
    let no_tui_invalid =
        crate::chat::no_tui::resolve_slash_for_delivery(router.as_ref(), "/bad::name");
    assert_eq!(tui_invalid, no_tui_invalid);
    assert!(matches!(
        tui_invalid,
        Err(sdk::CommandParseError::InvalidName { .. })
    ));
}

fn _arc_contract(_: Arc<dyn sdk::CommandCatalogPort>, _: Arc<dyn sdk::CommandRouterPort>) {}
