#[test]
fn sdk_reexports_tools_owned_command_contract_without_a_second_dto() {
    fn accept_catalog(_: std::sync::Arc<dyn sdk::CommandCatalogPort>) {}
    fn accept_router(_: std::sync::Arc<dyn sdk::CommandRouterPort>) {}

    let wiring = tools::composition::wire_commands(Vec::new()).expect("command wiring");
    accept_catalog(wiring.catalog());
    accept_router(wiring.router());
}

#[test]
fn sdk_command_descriptor_is_the_tools_published_language() {
    let descriptor: sdk::CommandDescriptor = tools::CommandDescriptor::new(
        "help",
        &[],
        "help",
        tools::CommandMechanism::SnapshotQuery,
        tools::CommandTarget::ApplicationShell,
        tools::CommandArgumentSchema::None,
    )
    .expect("descriptor");

    assert_eq!(descriptor.name.as_str(), "help");
}
