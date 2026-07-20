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

fn _arc_contract(_: Arc<dyn sdk::CommandCatalogPort>, _: Arc<dyn sdk::CommandRouterPort>) {}
