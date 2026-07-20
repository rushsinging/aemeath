use super::api;
use super::test_log;

#[test]
fn init_success_emits_enter_then_ok() {
    let dir = tempfile::tempdir().expect("temp dir");
    test_log::begin();
    let result = api::file_system_blob(dir.path());
    test_log::end();

    assert!(result.is_ok(), "construction should succeed");
    let logs = test_log::drain();
    let has_enter = logs.iter().any(|(_, message)| message.contains("enter"));
    let has_ok = logs
        .iter()
        .any(|(level, message)| *level <= log::Level::Info && message.contains("ok"));
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(
        has_ok,
        "expected an 'ok' log line at Info-or-lower, got {logs:?}"
    );
}

#[test]
fn init_failure_emits_enter_then_failed_at_error() {
    let file = tempfile::NamedTempFile::new().expect("temp file");
    test_log::begin();
    let result = api::file_system_blob(file.path().join("subdir"));
    test_log::end();

    assert!(result.is_err(), "construction should fail");
    let logs = test_log::drain();
    let has_enter = logs.iter().any(|(_, message)| message.contains("enter"));
    let has_failed = logs
        .iter()
        .any(|(level, message)| *level == log::Level::Error && message.contains("failed"));
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(
        has_failed,
        "expected a 'failed' log line at Error level, got {logs:?}"
    );
}

#[test]
fn init_logs_do_not_leak_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path_string = dir.path().to_string_lossy().into_owned();
    test_log::begin();
    let _ = api::file_system_blob(dir.path());
    test_log::end();

    for (_, message) in test_log::drain() {
        assert!(
            !message.contains(&path_string),
            "log line leaked the root path: {message:?}"
        );
    }
}
