use super::*;

#[test]
fn test_logging_config_default_filter_is_global_level() {
    let cfg = LoggingConfig::default();

    assert_eq!(cfg.to_filter_string(), "warn");
}

#[test]
fn test_logging_config_to_filter_string_uses_global_level() {
    let cfg = LoggingConfig {
        level: "info".to_string(),
        ..Default::default()
    };

    assert_eq!(cfg.to_filter_string(), "info");
}

#[test]
fn test_logging_config_deserializes_legacy_default_level_alias() {
    let cfg: LoggingConfig = serde_json::from_str(r#"{"default_level":"debug"}"#)
        .expect("legacy default_level should deserialize");

    assert_eq!(cfg.to_filter_string(), "debug");
}

#[test]
fn test_logging_config_to_level_filter_parses_valid_level() {
    let cfg = LoggingConfig {
        level: "debug".to_string(),
        ..Default::default()
    };

    assert_eq!(cfg.to_level_filter(), log::LevelFilter::Debug);
}

#[test]
fn test_logging_config_to_level_filter_accepts_warning_alias() {
    let cfg = LoggingConfig {
        level: "warning".to_string(),
        ..Default::default()
    };

    assert_eq!(cfg.to_level_filter(), log::LevelFilter::Warn);
}

#[test]
fn test_logging_config_to_level_filter_falls_back_to_warn() {
    let cfg = LoggingConfig {
        level: "invalid".to_string(),
        ..Default::default()
    };

    assert_eq!(cfg.to_level_filter(), log::LevelFilter::Warn);
}

#[test]
fn test_sub_agent_log_default() {
    let cfg = SubAgentLogConfig::default();
    assert!(cfg.enabled);
    assert!(cfg.include_request_payload);
    assert_eq!(cfg.max_payload_bytes, 65536);
}
