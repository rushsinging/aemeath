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

#[test]
fn eligible_skill_exposes_only_explicit_slash_name_and_aliases() {
    let skills = std::collections::HashMap::from([(
        "review".to_string(),
        sdk::SkillView {
            name: "review".to_string(),
            aliases: vec!["code-review".to_string()],
            slash_command: Some("review".to_string()),
            slash_aliases: vec!["cr".to_string()],
            description: "Review changes".to_string(),
            argument_hint: None,
        },
    )]);

    let wiring = composition::tools::wire_commands_with_skills(&skills)
        .expect("eligible Skill must register a Slash command");

    assert!(matches!(
        wiring.router().resolve(sdk::SlashInput::new("/review")),
        Ok(sdk::CommandRoute::SkillRequest(_))
    ));
    match wiring.router().resolve(sdk::SlashInput::new("/cr staged")) {
        Ok(sdk::CommandRoute::SkillRequest(command)) => {
            assert_eq!(command.skill, "review");
            assert_eq!(command.arguments.join(), "staged");
        }
        other => panic!("expected canonical SkillRequest, got {other:?}"),
    }
    assert!(matches!(
        wiring
            .router()
            .resolve(sdk::SlashInput::new("/code-review")),
        Err(sdk::CommandParseError::UnknownCommand { .. })
    ));
}

#[test]
fn malformed_slash_projection_does_not_block_command_catalog_bootstrap() {
    let skills = std::collections::HashMap::from([(
        "external-skill".to_string(),
        sdk::SkillView {
            name: "external-skill".to_string(),
            aliases: Vec::new(),
            slash_command: Some("bad::slash".to_string()),
            slash_aliases: Vec::new(),
            description: "External skill".to_string(),
            argument_hint: None,
        },
    )]);

    let wiring = composition::tools::wire_commands_with_skills(&skills)
        .expect("invalid external Slash projection must be skipped");

    assert!(matches!(
        wiring.router().resolve(sdk::SlashInput::new("/bad::slash")),
        Err(sdk::CommandParseError::InvalidName { .. })
    ));
}

#[test]
fn namespaced_skill_exposes_qualified_slash_command_without_short_alias() {
    let skills = std::collections::HashMap::from([(
        "superpowers:writing-plans".to_string(),
        sdk::SkillView {
            name: "superpowers:writing-plans".to_string(),
            aliases: vec!["writing-plans".to_string()],
            slash_command: Some("superpowers:writing-plans".to_string()),
            slash_aliases: Vec::new(),
            description: "Plan implementation work".to_string(),
            argument_hint: None,
        },
    )]);

    let wiring = composition::tools::wire_commands_with_skills(&skills)
        .expect("namespaced skill must register its qualified Slash command");

    match wiring
        .router()
        .resolve(sdk::SlashInput::new("/superpowers:writing-plans feature"))
    {
        Ok(sdk::CommandRoute::SkillRequest(command)) => {
            assert_eq!(command.skill, "superpowers:writing-plans");
            assert_eq!(command.arguments.as_slice(), ["feature"]);
        }
        other => panic!("expected qualified SkillRequest, got {other:?}"),
    }
    assert!(matches!(
        wiring
            .router()
            .resolve(sdk::SlashInput::new("/writing-plans")),
        Err(sdk::CommandParseError::UnknownCommand { .. })
    ));
}

#[test]
fn empty_skill_map_matches_builtin_wiring_and_builtin_conflicts_fail_closed() {
    let empty = std::collections::HashMap::new();
    let wiring = composition::tools::wire_commands_with_skills(&empty).unwrap();
    assert!(matches!(
        wiring.router().resolve(sdk::SlashInput::new("/help")),
        Ok(sdk::CommandRoute::SnapshotQuery { .. })
    ));

    let conflicting = std::collections::HashMap::from([(
        "conflict".to_string(),
        sdk::SkillView {
            name: "conflict".to_string(),
            aliases: Vec::new(),
            slash_command: Some("help".to_string()),
            slash_aliases: vec!["quit".to_string()],
            description: "conflict".to_string(),
            argument_hint: None,
        },
    )]);
    assert!(matches!(
        composition::tools::wire_commands_with_skills(&conflicting),
        Err(sdk::CommandParseError::DuplicateName { .. })
    ));
}
