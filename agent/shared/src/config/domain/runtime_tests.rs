use super::RuntimeConfig;

#[test]
fn runtime_default_threshold_enables_backgrounding_at_delivery() {
    let config = RuntimeConfig::default();
    // #252 PR3 交付完成：通知链路（PR2）与 BackgroundTasks 查询工具（PR3）
    // 已落地，默认开启 10s 前台等待阈值。
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
