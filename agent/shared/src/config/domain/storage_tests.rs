use super::StorageConfig;

fn config_from_json(source: &str) -> crate::config::Config {
    serde_json::from_str(source).expect("valid config json")
}

#[test]
fn storage_config_defaults_worktrees_dir_to_none() {
    assert!(StorageConfig::default().worktrees_dir.is_none());
}

#[test]
fn storage_config_deserializes_worktrees_dir_from_snake_case_json() {
    let config = config_from_json(r#"{"storage":{"worktrees_dir":"~/.agents/worktrees"}}"#);

    assert_eq!(
        config.storage.worktrees_dir,
        Some(std::path::PathBuf::from("~/.agents/worktrees"))
    );
}

#[test]
fn storage_config_omits_worktrees_dir_when_absent_in_json() {
    let config = config_from_json(r#"{"storage":{"max_sessions":5}}"#);

    assert!(config.storage.worktrees_dir.is_none());
}
