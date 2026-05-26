use super::*;
use serde_json::json;
use share::config::LoggingConfig;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[test]
fn test_log_file_file_name_happy_path() {
    assert_eq!(LogFile::Aemeath.file_name(), "aemeath.log");
    assert_eq!(LogFile::Agent.file_name(), "agent.log");
}

#[test]
fn test_log_file_file_name_boundary_all_variants() {
    let names = [
        LogFile::Aemeath.file_name(),
        LogFile::Agent.file_name(),
        LogFile::Panic.file_name(),
        LogFile::Input.file_name(),
        LogFile::Output.file_name(),
        LogFile::Tool.file_name(),
    ];
    assert_eq!(names.len(), 6);
    assert!(names.iter().all(|name| name.ends_with(".log")));
}

#[test]
fn test_format_text_line_happy_path() {
    let line = format_text_line("session-1", "INFO", "agent", "started");
    assert!(line.contains("[session:session-1]"));
    assert!(line.contains("[turn:-]"));
    assert!(line.contains("[INFO]"));
    assert!(line.ends_with("started"));
}

#[test]
fn test_format_text_line_with_turn_happy_path() {
    let line = format_text_line_with_turn("session-1", Some(3), "INFO", "agent", "started");
    assert!(line.contains("[session:session-1]"));
    assert!(line.contains("[turn:3]"));
}

#[test]
fn test_format_text_line_boundary_empty_values() {
    let line = format_text_line("", "", "", "");
    assert!(line.contains("[session:] [turn:-] [] []"));
}

#[test]
fn test_rotated_path_happy_path() {
    let path = PathBuf::from("/tmp/aemeath.log");
    assert_eq!(rotated_path(&path, 2), PathBuf::from("/tmp/aemeath.log.2"));
}

#[test]
fn test_is_rotated_log_path_happy_path() {
    assert!(is_rotated_log_path(Path::new("aemeath.log.1")));
    assert!(is_rotated_log_path(Path::new("agent.log.5")));
}

#[test]
fn test_is_rotated_log_path_error_non_numeric_suffix() {
    assert!(!is_rotated_log_path(Path::new("aemeath.log.old")));
    assert!(!is_rotated_log_path(Path::new("aemeath.log")));
}

#[test]
fn test_log_path_uses_agents_logs_dir() {
    let temp_agents_dir = std::env::temp_dir().join(format!(
        "aemeath_log_path_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let previous = std::env::var_os(share::config::paths::AGENTS_DIR_ENV);
    std::env::set_var(share::config::paths::AGENTS_DIR_ENV, &temp_agents_dir);

    assert_eq!(
        log_path(LogFile::Aemeath),
        temp_agents_dir.join("logs").join("aemeath.log")
    );

    if let Some(previous) = previous {
        std::env::set_var(share::config::paths::AGENTS_DIR_ENV, previous);
    } else {
        std::env::remove_var(share::config::paths::AGENTS_DIR_ENV);
    }
}

#[test]
fn test_json_logger_log_input_happy_path_writes_user_message() {
    let temp = std::env::temp_dir().join(format!(
        "aemeath-json-logger-test-{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&temp).unwrap();
    let mut logger = JsonLogger::new("session-1", &temp, &LoggingConfig::default()).unwrap();

    logger
        .log_input(
            1,
            "default",
            "model-1",
            json!({"messages":[{"role":"user","content":"hello"}]}),
        )
        .unwrap();

    let content = fs::read_to_string(temp.join("input.log")).unwrap();
    assert!(content.contains("\"session\":\"session-1\""));
    assert!(content.contains("\"type\":\"input\""));
    assert!(content.contains("hello"));
}
