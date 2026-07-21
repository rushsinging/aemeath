use std::sync::Arc;

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
            description: Some("Review".to_string()),
            content: "review content".to_string(),
            source: None,
        },
    )]);
    let wiring = composition::tools::wire_commands_with_skills(&skills).unwrap();
    let router = wiring.router();

    let tui = crate::tui::app::slash::resolve_slash_for_delivery(router.as_ref(), "/cr staged");
    let no_tui = crate::chat::no_tui::resolve_slash_for_delivery(router.as_ref(), "/cr staged");
    assert_eq!(tui, no_tui);
    assert!(matches!(
        tui,
        Ok(sdk::CommandRoute::PromptInjection(command))
            if command.command.as_str() == "review" && command.arguments.as_slice() == ["staged"]
    ));

    let tui_invalid =
        crate::tui::app::slash::resolve_slash_for_delivery(router.as_ref(), "/bad:name");
    let no_tui_invalid =
        crate::chat::no_tui::resolve_slash_for_delivery(router.as_ref(), "/bad:name");
    assert_eq!(tui_invalid, no_tui_invalid);
    assert!(matches!(
        tui_invalid,
        Err(sdk::CommandParseError::InvalidName { .. })
    ));
}

fn _arc_contract(_: Arc<dyn sdk::CommandCatalogPort>, _: Arc<dyn sdk::CommandRouterPort>) {}
