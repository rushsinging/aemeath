use super::RuntimeConfig;

#[test]
fn runtime_default_threshold_is_ten_seconds() {
    let config = RuntimeConfig::default();
    assert_eq!(config.tool_background_threshold_secs, 10);
}

#[test]
fn runtime_serde_missing_section_falls_back_to_default() {
    let document: RuntimeConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(document.tool_background_threshold_secs, 10);
}

#[test]
fn runtime_zero_threshold_is_preserved_as_disabled() {
    let document: RuntimeConfig =
        serde_json::from_str(r#"{"toolBackgroundThresholdSecs":0}"#).unwrap();
    assert_eq!(
        document.tool_background_threshold_secs, 0,
        "0 是合法的禁用值，不得被归一化为默认值"
    );
}

#[test]
fn runtime_serde_accepts_snake_case_and_camel_case_aliases() {
    let snake: RuntimeConfig =
        serde_json::from_str(r#"{"tool_background_threshold_secs":30}"#).unwrap();
    let camel: RuntimeConfig =
        serde_json::from_str(r#"{"toolBackgroundThresholdSecs":45}"#).unwrap();
    assert_eq!(snake.tool_background_threshold_secs, 30);
    assert_eq!(camel.tool_background_threshold_secs, 45);
}
