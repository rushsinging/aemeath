use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;

#[test]
fn tool_selection_defaults_to_allowing_every_tool() {
    let selection = ConfigSnapshot::new(Config::default()).tool_selection();

    assert!(selection.allows("AskUserQuestion"));
    assert!(selection.allows("Read"));
}

#[test]
fn tool_selection_uses_enabled_as_allowlist_and_disabled_wins_case_insensitively() {
    let mut config = Config::default();
    config.tools.enabled = vec![
        " Read ".to_string(),
        "ASKUSERQUESTION".to_string(),
        "read".to_string(),
        " ".to_string(),
    ];
    config.tools.disabled = vec![
        " askuserquestion ".to_string(),
        "ASKUSERQUESTION".to_string(),
        " ".to_string(),
    ];

    let selection = ConfigSnapshot::new(config).tool_selection();

    assert!(selection.allows("READ"));
    assert!(!selection.allows("AskUserQuestion"));
    assert!(!selection.allows("Bash"));
    assert_eq!(selection.enabled(), vec!["read"]);
    assert_eq!(selection.disabled(), vec!["askuserquestion"]);
}

#[test]
fn tool_selection_keeps_allowlist_active_when_disabled_removes_its_only_entry() {
    let mut config = Config::default();
    config.tools.enabled = vec!["AskUserQuestion".to_string()];
    config.tools.disabled = vec!["askuserquestion".to_string()];

    let selection = ConfigSnapshot::new(config).tool_selection();

    assert!(!selection.allows("AskUserQuestion"));
    assert!(!selection.allows("Read"));
}

#[test]
fn tool_selection_disabled_applies_without_an_allowlist() {
    let mut config = Config::default();
    config.tools.disabled = vec!["AskUserQuestion".to_string()];

    let selection = ConfigSnapshot::new(config).tool_selection();

    assert!(!selection.allows("askuserquestion"));
    assert!(selection.allows("Read"));
}
