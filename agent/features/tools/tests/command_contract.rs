use tools::{
    ApplicationControlTarget, CommandArgumentSchema, CommandMechanism, CommandParseError,
    CommandRoute, CommandTarget, SlashInput,
};

#[test]
fn catalog_is_the_single_source_for_discovery_and_alias_completion() {
    let wiring = tools::composition::wire_commands(Vec::new()).expect("valid builtin catalog");
    let descriptors = wiring.catalog().list();

    assert!(descriptors
        .iter()
        .any(|command| command.name.as_str() == "help"));
    assert!(descriptors.iter().any(|command| {
        command.name.as_str() == "exit"
            && command.aliases.iter().any(|alias| alias.as_str() == "quit")
    }));
    assert!(wiring
        .catalog()
        .complete("/qu")
        .iter()
        .any(|completion| completion.replacement == "/quit"));
}

#[test]
fn router_classifies_all_three_mechanisms_without_executing() {
    let review = tools::CommandDescriptor::new(
        "review",
        &[],
        "Review changes",
        tools::CommandMechanism::PromptInjection,
        tools::CommandTarget::ContextManagement,
        tools::CommandArgumentSchema::OptionalText,
    )
    .expect("review descriptor");
    let wiring = tools::composition::wire_commands(vec![review]).expect("valid command catalog");

    assert!(matches!(
        wiring.router().resolve(SlashInput::new("/review staged")),
        Ok(CommandRoute::PromptInjection(command))
            if command.command.as_str() == "review"
                && command.arguments.as_slice() == ["staged"]
    ));
    assert!(matches!(
        wiring.router().resolve(SlashInput::new("/reflect 3")),
        Ok(CommandRoute::SnapshotQuery { command, .. })
            if command.command.as_str() == "reflect"
                && command.arguments.as_slice() == ["3"]
    ));
    assert!(matches!(
        wiring.router().resolve(SlashInput::new("/compact")),
        Ok(CommandRoute::ApplicationControl {
            target: ApplicationControlTarget::ContextManagement,
            ..
        })
    ));
}

#[test]
fn router_rejects_unknown_commands_and_invalid_arguments() {
    let wiring = tools::composition::wire_commands(Vec::new()).expect("valid builtin catalog");

    assert!(matches!(
        wiring.router().resolve(SlashInput::new("/does-not-exist")),
        Err(CommandParseError::UnknownCommand { .. })
    ));
    assert!(matches!(
        wiring.router().resolve(SlashInput::new("/reflect 0")),
        Err(CommandParseError::InvalidArgument { .. })
    ));
    assert!(matches!(
        wiring.router().resolve(SlashInput::new("/compact extra")),
        Err(CommandParseError::UnexpectedArguments { .. })
    ));
}

#[test]
fn delivery_commands_have_an_explicit_application_shell_target() {
    let wiring = tools::composition::wire_commands(Vec::new()).expect("valid builtin catalog");
    let help = wiring
        .catalog()
        .list()
        .into_iter()
        .find(|command| command.name.as_str() == "help")
        .expect("help descriptor");

    assert_eq!(help.mechanism, CommandMechanism::SnapshotQuery);
    assert_eq!(help.target, CommandTarget::ApplicationShell);
    assert_eq!(help.argument_schema, CommandArgumentSchema::None);
}

#[test]
fn builtin_catalog_exposes_the_complete_stable_descriptor_matrix() {
    // 更新 builtin Command 时必须同步此稳定的公开目录契约。
    let wiring = tools::composition::wire_commands(Vec::new()).expect("valid builtin catalog");
    let commands = wiring.catalog().list();
    let names = commands
        .iter()
        .map(|command| command.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "clear",
            "clear-images",
            "compact",
            "config",
            "context",
            "cost",
            "doctor",
            "exit",
            "help",
            "images",
            "init",
            "memory",
            "model",
            "paste",
            "reflect",
            "resume",
            "rewind",
            "save",
            "session",
            "stats",
            "status",
            "update",
            "usage",
            "version",
        ]
    );
    assert!(commands.iter().all(|command| match command.mechanism {
        CommandMechanism::PromptInjection => true,
        CommandMechanism::SnapshotQuery =>
            command.target != CommandTarget::ApplicationVersionControl,
        CommandMechanism::ApplicationControl => {
            command.target != CommandTarget::Audit && command.target != CommandTarget::Provider
        }
    }));
    let reflect = commands
        .iter()
        .find(|command| command.name.as_str() == "reflect")
        .unwrap();
    assert_eq!(
        reflect.argument_schema,
        CommandArgumentSchema::OptionalPositiveUsize { default: 10 }
    );
}

#[test]
fn public_wiring_preserves_duplicate_target_and_missing_argument_errors() {
    let duplicate = tools::CommandDescriptor::new(
        "help",
        &[],
        "duplicate",
        CommandMechanism::PromptInjection,
        CommandTarget::ContextManagement,
        CommandArgumentSchema::OptionalText,
    )
    .unwrap();
    assert!(matches!(
        tools::composition::wire_commands(vec![duplicate]),
        Err(CommandParseError::DuplicateName { .. })
    ));

    let mismatch = tools::CommandDescriptor::new(
        "bad-target",
        &[],
        "bad",
        CommandMechanism::ApplicationControl,
        CommandTarget::Audit,
        CommandArgumentSchema::None,
    )
    .unwrap();
    assert!(matches!(
        tools::composition::wire_commands(vec![mismatch]),
        Err(CommandParseError::TargetMismatch { .. })
    ));

    let wiring = tools::composition::wire_commands(Vec::new()).unwrap();
    assert!(matches!(
        wiring.router().resolve(SlashInput::new("/resume")),
        Err(CommandParseError::MissingArgument { .. })
    ));
}
