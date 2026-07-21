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
            description: Some("Review changes".to_string()),
            content: "skill content".to_string(),
            source: Some("/skills/review/SKILL.md".to_string()),
        },
    )]);

    let wiring = composition::tools::wire_commands_with_skills(&skills)
        .expect("eligible Skill must register a Slash command");

    assert!(matches!(
        wiring.router().resolve(sdk::SlashInput::new("/review")),
        Ok(sdk::CommandRoute::PromptInjection(_))
    ));
    assert!(matches!(
        wiring.router().resolve(sdk::SlashInput::new("/cr")),
        Ok(sdk::CommandRoute::PromptInjection(_))
    ));
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
            slash_command: Some("bad:slash".to_string()),
            slash_aliases: Vec::new(),
            description: Some("External skill".to_string()),
            content: "skill content".to_string(),
            source: Some("/skills/external/SKILL.md".to_string()),
        },
    )]);

    let wiring = composition::tools::wire_commands_with_skills(&skills)
        .expect("invalid external Slash projection must be skipped");

    assert!(matches!(
        wiring.router().resolve(sdk::SlashInput::new("/bad:slash")),
        Err(sdk::CommandParseError::InvalidName { .. })
    ));
}

#[test]
fn namespaced_skill_remains_available_without_becoming_a_slash_command() {
    let skills = std::collections::HashMap::from([(
        "superpowers:writing-plans".to_string(),
        sdk::SkillView {
            name: "superpowers:writing-plans".to_string(),
            aliases: vec!["writing-plans".to_string()],
            slash_command: None,
            slash_aliases: Vec::new(),
            description: Some("Plan implementation work".to_string()),
            content: "skill content".to_string(),
            source: Some("/skills/superpowers/writing-plans/SKILL.md".to_string()),
        },
    )]);

    let wiring = composition::tools::wire_commands_with_skills(&skills)
        .expect("namespaced skill must not prevent command catalog bootstrap");

    assert!(wiring
        .catalog()
        .list()
        .iter()
        .all(|command| command.name.as_str() != "superpowers:writing-plans"));
    assert!(matches!(
        wiring
            .router()
            .resolve(sdk::SlashInput::new("/writing-plans")),
        Err(sdk::CommandParseError::UnknownCommand { .. })
    ));
}
