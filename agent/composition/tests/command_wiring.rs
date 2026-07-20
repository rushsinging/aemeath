#[test]
fn composition_exposes_one_command_catalog_and_router_pair() {
    let wiring = composition::tools::wire_commands().expect("command wiring");
    let commands = wiring.catalog().list();

    assert!(commands
        .iter()
        .any(|command| command.name.as_str() == "help"));
    assert!(matches!(
        wiring.router().resolve(sdk::SlashInput::new("/compact")),
        Ok(sdk::CommandRoute::ApplicationControl { .. })
    ));
}
