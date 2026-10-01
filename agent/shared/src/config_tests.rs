use super::*;

#[test]
fn test_default_config() {
    let config = Config::default();
    assert_eq!(config.model.name, "claude-sonnet-4-6");
    assert_eq!(config.model.max_tokens, 8192);
    assert!(config.ui.markdown);
    assert!(config.storage.persist_sessions);
    assert!(config.memory.enabled);
    assert_eq!(config.memory.max_entries, 100);
}
