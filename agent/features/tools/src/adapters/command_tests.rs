use crate::adapters::command::CommandAdapter;
use crate::domain::{
    ApplicationControlTarget, CommandArgumentSchema, CommandCatalogPort, CommandDescriptor,
    CommandMechanism, CommandParseError, CommandRoute, CommandRouterPort, CommandTarget,
    SlashInput, SnapshotQueryTarget,
};

fn descriptor(
    name: &str,
    aliases: &[&str],
    mechanism: CommandMechanism,
    target: CommandTarget,
    schema: CommandArgumentSchema,
) -> CommandDescriptor {
    CommandDescriptor::new(name, aliases, name, mechanism, target, schema).unwrap()
}

#[test]
fn adapter_rejects_duplicate_names_aliases_and_invalid_target_pairs() {
    let duplicate = CommandAdapter::try_new(vec![
        descriptor(
            "help",
            &[],
            CommandMechanism::SnapshotQuery,
            CommandTarget::ApplicationShell,
            CommandArgumentSchema::None,
        ),
        descriptor(
            "other",
            &["help"],
            CommandMechanism::PromptInjection,
            CommandTarget::ContextManagement,
            CommandArgumentSchema::OptionalText,
        ),
    ]);
    assert!(matches!(
        duplicate,
        Err(CommandParseError::DuplicateName { .. })
    ));

    let mismatch = CommandAdapter::try_new(vec![descriptor(
        "bad",
        &[],
        CommandMechanism::ApplicationControl,
        CommandTarget::Audit,
        CommandArgumentSchema::None,
    )]);
    assert!(matches!(
        mismatch,
        Err(CommandParseError::TargetMismatch { .. })
    ));
}

#[test]
fn completion_is_case_insensitive_sorted_and_has_no_duplicate_aliases() {
    let adapter = CommandAdapter::try_new(vec![
        descriptor(
            "zoom",
            &["z"],
            CommandMechanism::PromptInjection,
            CommandTarget::ContextManagement,
            CommandArgumentSchema::OptionalText,
        ),
        descriptor(
            "alpha",
            &["a"],
            CommandMechanism::PromptInjection,
            CommandTarget::ContextManagement,
            CommandArgumentSchema::OptionalText,
        ),
    ])
    .unwrap();

    let completions = adapter.complete(" /A ");
    assert_eq!(
        completions
            .iter()
            .map(|item| item.replacement.as_str())
            .collect::<Vec<_>>(),
        vec!["/alpha", "/a"]
    );
    assert!(adapter.complete("/missing").is_empty());
}

#[test]
fn router_validates_required_and_positive_arguments_and_maps_targets() {
    let adapter = CommandAdapter::try_new(vec![
        descriptor(
            "resume",
            &[],
            CommandMechanism::ApplicationControl,
            CommandTarget::ContextManagement,
            CommandArgumentSchema::RequiredText,
        ),
        descriptor(
            "reflect",
            &[],
            CommandMechanism::SnapshotQuery,
            CommandTarget::Memory,
            CommandArgumentSchema::OptionalPositiveUsize { default: 10 },
        ),
        descriptor(
            "status",
            &[],
            CommandMechanism::SnapshotQuery,
            CommandTarget::Runtime,
            CommandArgumentSchema::None,
        ),
        descriptor(
            "review",
            &[],
            CommandMechanism::PromptInjection,
            CommandTarget::ContextManagement,
            CommandArgumentSchema::OptionalText,
        ),
        descriptor(
            "model",
            &[],
            CommandMechanism::ApplicationControl,
            CommandTarget::Config,
            CommandArgumentSchema::OptionalText,
        ),
    ])
    .unwrap();

    assert!(matches!(
        adapter.resolve(SlashInput::new("/resume")),
        Err(CommandParseError::MissingArgument { .. })
    ));
    assert!(matches!(
        adapter.resolve(SlashInput::new("/reflect 0")),
        Err(CommandParseError::InvalidArgument { .. })
    ));
    assert!(matches!(
        adapter.resolve(SlashInput::new("/reflect 1 2")),
        Err(CommandParseError::UnexpectedArguments { .. })
    ));
    assert!(matches!(
        adapter.resolve(SlashInput::new("/reflect")),
        Ok(CommandRoute::SnapshotQuery { command, target: SnapshotQueryTarget::Memory })
            if command.arguments.as_slice() == ["10"]
    ));
    assert!(matches!(
        adapter.resolve(SlashInput::new(" /status ")),
        Ok(CommandRoute::SnapshotQuery {
            target: SnapshotQueryTarget::Runtime,
            ..
        })
    ));
    assert!(matches!(
        adapter.resolve(SlashInput::new("/resume session-1")),
        Ok(CommandRoute::ApplicationControl {
            target: ApplicationControlTarget::ContextManagement,
            command,
        }) if command.arguments.as_slice() == ["session-1"]
    ));
    assert!(matches!(
        adapter.resolve(SlashInput::new("/review staged")),
        Ok(CommandRoute::PromptInjection(command))
            if command.command.as_str() == "review" && command.arguments.as_slice() == ["staged"]
    ));
    assert!(matches!(
        adapter.resolve(SlashInput::new("/model gpt-5")),
        Ok(CommandRoute::ApplicationControl { command, .. })
            if command.command.as_str() == "model" && command.arguments.as_slice() == ["gpt-5"]
    ));
    assert!(matches!(
        adapter.resolve(SlashInput::new("/model")),
        Ok(CommandRoute::ApplicationControl { command, .. })
            if command.command.as_str() == "model" && command.arguments.as_slice().is_empty()
    ));
}
