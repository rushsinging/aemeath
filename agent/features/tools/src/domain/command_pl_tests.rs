use super::command_pl::*;

#[test]
fn command_name_normalizes_trimmed_slash_prefixed_ascii_names() {
    for (raw, expected) in [
        (" help ", "help"),
        ("/HELP", "help"),
        ("my-command", "my-command"),
        ("my_command", "my_command"),
    ] {
        assert_eq!(CommandName::new(raw).unwrap().as_str(), expected);
    }
}

#[test]
fn command_name_rejects_empty_whitespace_and_non_ascii_or_punctuation() {
    for raw in ["", "   ", "/", "two words", "命令", "bad:name"] {
        assert!(matches!(
            CommandName::new(raw),
            Err(CommandParseError::InvalidName { .. })
        ));
    }
}

#[test]
fn descriptor_normalizes_aliases_and_rejects_invalid_alias() {
    let descriptor = CommandDescriptor::new(
        "Review",
        &["/CR"],
        "Review",
        CommandMechanism::PromptInjection,
        CommandTarget::ContextManagement,
        CommandArgumentSchema::OptionalText,
    )
    .unwrap();
    assert_eq!(descriptor.name.as_str(), "review");
    assert_eq!(descriptor.aliases[0].as_str(), "cr");

    assert!(matches!(
        CommandDescriptor::new(
            "review",
            &["bad:alias"],
            "Review",
            CommandMechanism::PromptInjection,
            CommandTarget::ContextManagement,
            CommandArgumentSchema::OptionalText,
        ),
        Err(CommandParseError::InvalidName { .. })
    ));
}

#[test]
fn parsed_arguments_preserve_tokens_and_join_with_spaces() {
    let arguments = ParsedArguments::new(vec!["one".into(), "two".into()]);
    assert_eq!(arguments.as_slice(), ["one", "two"]);
    assert_eq!(arguments.join(), "one two");
    assert_eq!(ParsedArguments::new(vec![]).join(), "");
}

#[test]
fn command_pl_serializes_descriptor_and_error_messages_remain_actionable() {
    let descriptor = CommandDescriptor::new(
        "reflect",
        &[],
        "Reflect",
        CommandMechanism::SnapshotQuery,
        CommandTarget::Memory,
        CommandArgumentSchema::OptionalPositiveUsize { default: 10 },
    )
    .unwrap();
    let encoded = serde_json::to_string(&descriptor).unwrap();
    let decoded: CommandDescriptor = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, descriptor);
    assert!(CommandParseError::MissingArgument {
        command: "resume".into()
    }
    .to_string()
    .contains("/resume"));
}
