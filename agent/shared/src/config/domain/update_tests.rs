use super::*;

#[test]
fn test_default_config() {
    let config = UpdateConfig::default();
    assert!(config.check_on_startup);
    assert_eq!(config.channel, "stable");
}

#[test]
fn test_deserialize_partial() {
    let json = r#"{"check_on_startup": false}"#;
    let config: UpdateConfig = serde_json::from_str(json).unwrap();
    assert!(!config.check_on_startup);
    assert_eq!(config.channel, "stable"); // 默认值
}

#[test]
fn test_deserialize_empty() {
    let json = r#"{}"#;
    let config: UpdateConfig = serde_json::from_str(json).unwrap();
    assert!(config.check_on_startup); // 默认值
    assert_eq!(config.channel, "stable"); // 默认值
}
