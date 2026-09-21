use super::test_log;

#[test]
fn file_system_blob_init_success_emits_enter_then_ok() {
    let dir = tempfile::tempdir().expect("temp dir");
    let capture = test_log::begin();
    let result = super::file_system_blob(dir.path());
    drop(capture);

    assert!(result.is_ok(), "construction should succeed");
    let logs = test_log::drain();
    assert!(!logs.is_empty(), "successful initialization must emit logs");
    let has_enter = logs.iter().any(|(level, message)| {
        *level == log::Level::Debug && message == "file_system_blob init enter"
    });
    let has_ok = logs.iter().any(|(level, message)| {
        *level == log::Level::Info && message == "file_system_blob init ok"
    });
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(has_ok, "expected an Info-level 'ok' log line, got {logs:?}");
}

#[test]
fn file_system_blob_init_failure_emits_enter_then_failed_at_error() {
    let file = tempfile::NamedTempFile::new().expect("temp file");
    let capture = test_log::begin();
    let result = super::file_system_blob(file.path().join("subdir"));
    drop(capture);

    assert!(result.is_err(), "construction should fail");
    let logs = test_log::drain();
    assert!(!logs.is_empty(), "failed initialization must emit logs");
    let has_enter = logs.iter().any(|(level, message)| {
        *level == log::Level::Debug && message == "file_system_blob init enter"
    });
    let has_failed = logs.iter().any(|(level, message)| {
        *level == log::Level::Error && message == "file_system_blob init failed"
    });
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(
        has_failed,
        "expected a 'failed' log line at Error level, got {logs:?}"
    );
}

#[test]
fn file_system_blob_init_logs_do_not_leak_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path_string = dir.path().to_string_lossy().into_owned();
    let capture = test_log::begin();
    let _ = super::file_system_blob(dir.path());
    drop(capture);

    let logs = test_log::drain();
    assert!(!logs.is_empty(), "initialization must emit logs");
    for (_, message) in logs {
        assert!(
            !message.contains(&path_string),
            "log line leaked the root path: {message:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn file_system_blob_returns_working_port() {
    let dir = tempfile::tempdir().expect("temp dir");
    let blob = super::file_system_blob(dir.path()).expect("blob port");

    let entries = blob
        .list_primary(super::StorageNamespace::Session)
        .await
        .expect("list primary on fresh root");
    assert!(
        entries.is_empty(),
        "fresh root must expose no primary entries"
    );
}

#[test]
fn file_system_dataset_init_success_emits_enter_then_ok() {
    let dir = tempfile::tempdir().expect("temp dir");
    let capture = test_log::begin();
    let result = super::file_system_dataset(dir.path());
    drop(capture);

    assert!(result.is_ok(), "construction should succeed");
    let logs = test_log::drain();
    assert!(!logs.is_empty(), "successful initialization must emit logs");
    let has_enter = logs.iter().any(|(level, message)| {
        *level == log::Level::Debug && message == "file_system_dataset init enter"
    });
    let has_ok = logs.iter().any(|(level, message)| {
        *level == log::Level::Info && message == "file_system_dataset init ok"
    });
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(has_ok, "expected an Info-level 'ok' log line, got {logs:?}");
}

#[test]
fn file_system_dataset_init_failure_emits_enter_then_failed_at_error() {
    let file = tempfile::NamedTempFile::new().expect("temp file");
    let capture = test_log::begin();
    let result = super::file_system_dataset(file.path().join("subdir"));
    drop(capture);

    assert!(result.is_err(), "construction should fail");
    let logs = test_log::drain();
    assert!(!logs.is_empty(), "failed initialization must emit logs");
    let has_enter = logs.iter().any(|(level, message)| {
        *level == log::Level::Debug && message == "file_system_dataset init enter"
    });
    let has_failed = logs.iter().any(|(level, message)| {
        *level == log::Level::Error && message == "file_system_dataset init failed"
    });
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(
        has_failed,
        "expected a 'failed' log line at Error level, got {logs:?}"
    );
}

#[test]
fn file_system_dataset_init_logs_do_not_leak_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path_string = dir.path().to_string_lossy().into_owned();
    let capture = test_log::begin();
    let _ = super::file_system_dataset(dir.path());
    drop(capture);

    let logs = test_log::drain();
    assert!(!logs.is_empty(), "initialization must emit logs");
    for (_, message) in logs {
        assert!(
            !message.contains(&path_string),
            "log line leaked the root path: {message:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn file_system_dataset_returns_working_port() {
    let dir = tempfile::tempdir().expect("temp dir");
    let dataset = super::file_system_dataset(dir.path()).expect("dataset port");

    let datasets = dataset
        .list_datasets(super::StorageNamespace::Memory)
        .await
        .expect("list datasets on fresh root");
    assert!(datasets.is_empty(), "fresh root must expose no datasets");
}
