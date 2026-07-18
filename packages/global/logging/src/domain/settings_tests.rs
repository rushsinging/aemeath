use super::{LoggingOutputMode, LoggingSettings};
use log::LevelFilter;
use std::path::PathBuf;

#[test]
fn settings_preserve_static_path_rotation_retention_and_mode() {
    let settings = LoggingSettings::new(
        "aemeath:tui=debug,aemeath:agent:runtime=trace".to_string(),
        LoggingOutputMode::Stderr,
        PathBuf::from("/tmp/aemeath-logs"),
        42,
        3,
        14,
    );

    assert_eq!(
        settings.filter_directive(),
        "aemeath:tui=debug,aemeath:agent:runtime=trace"
    );
    assert_eq!(settings.max_level(), LevelFilter::Trace);
    assert_eq!(settings.output_mode(), LoggingOutputMode::Stderr);
    assert_eq!(settings.logs_dir(), PathBuf::from("/tmp/aemeath-logs"));
    assert_eq!(settings.max_bytes(), 42);
    assert_eq!(settings.max_backups(), 3);
    assert_eq!(settings.retention_days(), 14);
}

#[test]
fn global_and_per_target_directives_compute_one_consistent_max_level() {
    let global = LoggingSettings::new(
        "info".to_string(),
        LoggingOutputMode::File,
        PathBuf::from("logs"),
        1,
        1,
        1,
    );
    let per_target = LoggingSettings::new(
        "warn,aemeath:tui=debug,aemeath:agent:runtime=trace".to_string(),
        LoggingOutputMode::File,
        PathBuf::from("logs"),
        1,
        1,
        1,
    );

    assert_eq!(global.max_level(), LevelFilter::Info);
    assert_eq!(per_target.max_level(), LevelFilter::Trace);
}

#[test]
fn target_only_directive_remains_valid_and_opens_target_to_trace() {
    let settings = LoggingSettings::new(
        "aemeath:tui".to_string(),
        LoggingOutputMode::File,
        PathBuf::from("logs"),
        1,
        1,
        1,
    );

    assert_eq!(settings.filter_directive(), "aemeath:tui");
    assert_eq!(settings.max_level(), LevelFilter::Trace);
}

#[test]
fn message_regex_directive_is_preserved_and_uses_filter_level() {
    let settings = LoggingSettings::new(
        "aemeath=debug/foo,bar=invalid".to_string(),
        LoggingOutputMode::File,
        PathBuf::from("logs"),
        1,
        1,
        1,
    );
    assert_eq!(settings.filter_directive(), "aemeath=debug/foo,bar=invalid");
    assert_eq!(settings.max_level(), LevelFilter::Debug);
}

#[test]
fn zero_max_bytes_is_normalized_to_one() {
    let settings = LoggingSettings::new(
        "info".to_string(),
        LoggingOutputMode::File,
        PathBuf::from("logs"),
        0,
        1,
        1,
    );

    assert_eq!(settings.max_bytes(), 1);
}

#[test]
fn invalid_level_falls_back_to_warn_without_opening_filter() {
    let settings = LoggingSettings::new(
        "aemeath:tui=not-a-level".to_string(),
        LoggingOutputMode::File,
        PathBuf::from("logs"),
        1,
        1,
        1,
    );

    assert_eq!(settings.filter_directive(), "warn");
    assert_eq!(settings.max_level(), LevelFilter::Warn);
}
