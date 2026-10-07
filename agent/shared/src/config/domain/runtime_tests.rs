use super::RuntimeConfig;

#[test]
fn runtime_default_threshold_disables_backgrounding_pending_real_usage() {
    let config = RuntimeConfig::default();
    // #252 全套链路已交付（PR1-3）但默认暂关闭（2026-10-07 用户拍板）：
    // 真实使用验证后再开启 10s；显式配置 >0 可提前启用。
    assert_eq!(config.tool_background_threshold_secs, 0);
}

#[test]
fn runtime_serde_missing_section_falls_back_to_default() {
    let document: RuntimeConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(document.tool_background_threshold_secs, 0);
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
