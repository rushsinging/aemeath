//! crate 根 wiring 测试（自 application_tests.rs 迁出，随 wiring 归位 lib.rs）。
use super::*;

// The boundary must:
//   1. Emit a **debug** "enter" record on entry.
//   2. Emit an **info/debug** "success" record when the Result is Ok.
//   3. Emit a **warn** "failure" record when the Result is Err.
//   4. Never leak sensitive config values (e.g. api_key) into any
//      log message.
//   5. Return the original error unchanged (behavior preserved).
// ─────────────────────────────────────────────────────────────────

thread_local! {
    static CAPTURED_CONFIG_LOGS: std::cell::RefCell<Vec<(log::Level, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct ConfigCapturingLogger;

impl log::Log for ConfigCapturingLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if record.target().starts_with("aemeath:agent:config") {
            CAPTURED_CONFIG_LOGS.with(|cell| {
                cell.borrow_mut()
                    .push((record.level(), format!("{}", record.args())))
            });
        }
    }

    fn flush(&self) {}
}

/// Installs the capturing logger exactly once per test process. Safe to
/// call from every test: `log::set_logger` succeeds only once; later
/// calls are no-ops via `Once`. Capture storage is thread-local so
/// parallel tests never observe each other's records.
fn install_config_capturing_logger() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        log::set_boxed_logger(Box::new(ConfigCapturingLogger))
            .expect("capturing logger must install exactly once per process");
        log::set_max_level(log::LevelFilter::Trace);
    });
}

fn drain_captured_config_logs() -> Vec<(log::Level, String)> {
    CAPTURED_CONFIG_LOGS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// RAII guard that temporarily points `AEMEATH_AGENTS_DIR` at an
/// isolated tempdir so the real assembly entry never touches the
/// developer's `~/.agents` during tests.
static AGENTS_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct AgentsDirEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
}

fn test_native_store(root: &std::path::Path) -> NativeConfigStore {
    native_override_store(
        storage::file_system_blob(root.join("config-overrides"))
            .expect("create test config override blob"),
    )
}

impl AgentsDirEnvGuard {
    fn new() -> Self {
        let lock = AGENTS_DIR_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("AEMEATH_AGENTS_DIR", dir.path());
        }
        Self {
            _lock: lock,
            _dir: dir,
        }
    }
}

impl Drop for AgentsDirEnvGuard {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var("AEMEATH_AGENTS_DIR");
        }
    }
}

#[tokio::test]
async fn wire_entry_logs_debug_enter_then_success_on_ok() {
    install_config_capturing_logger();
    drain_captured_config_logs();
    let _env = AgentsDirEnvGuard::new();
    let project = tempfile::tempdir().unwrap();

    let result = wire_project_config_with_cli(
        project.path(),
        test_native_store(project.path()),
        crate::CliConfigInput::default(),
    )
    .await;

    assert!(
        result.is_ok(),
        "expected wire_project_config_with_cli to succeed"
    );
    let logs = drain_captured_config_logs();
    assert!(
        logs.iter()
            .any(|(l, m)| *l == log::Level::Debug && m.contains("enter")),
        "expected a debug 'enter' record; captured: {logs:?}"
    );
    assert!(
        logs.iter().any(|(l, m)| {
            (*l == log::Level::Info || *l == log::Level::Debug) && m.contains("success")
        }),
        "expected an info/debug 'success' record; captured: {logs:?}"
    );
    assert!(
        !logs.iter().any(|(l, _)| *l == log::Level::Warn),
        "no warn record expected on success; captured: {logs:?}"
    );
}

#[tokio::test]
async fn wire_entry_logs_debug_enter_then_warn_failure_on_err_and_returns_original_error() {
    install_config_capturing_logger();
    drain_captured_config_logs();

    let missing_store_root = tempfile::tempdir().unwrap();
    let result = wire_project_config_with_cli(
        std::path::Path::new("/nonexistent/config/does/not/exist"),
        test_native_store(missing_store_root.path()),
        crate::CliConfigInput::default(),
    )
    .await;

    let error = match result {
        Ok(_) => panic!("expected failure for nonexistent project dir"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        share::error::DomainError::from(crate::domain::ConfigError::InvalidLocation(
            crate::domain::ProjectConfigLocationError::NotCanonical,
        )),
        "original error must be returned unchanged"
    );
    let logs = drain_captured_config_logs();
    assert!(
        logs.iter()
            .any(|(l, m)| *l == log::Level::Debug && m.contains("enter")),
        "expected a debug 'enter' record; captured: {logs:?}"
    );
    assert!(
        logs.iter()
            .any(|(l, m)| *l == log::Level::Warn && m.contains("failure")),
        "expected a warn 'failure' record; captured: {logs:?}"
    );
    assert!(
        !logs.iter().any(|(_, m)| m.contains("success")),
        "no success record expected on failure; captured: {logs:?}"
    );
}

#[tokio::test]
async fn wire_entry_never_logs_sensitive_config_values() {
    install_config_capturing_logger();
    drain_captured_config_logs();
    let _env = AgentsDirEnvGuard::new();
    let project = tempfile::tempdir().unwrap();
    let secret = "do-not-leak-this-api-key-42";

    let _ = wire_project_config_with_cli(
        project.path(),
        test_native_store(project.path()),
        crate::adapters::CliConfigInput {
            api_key: Some(secret.into()),
            ..Default::default()
        },
    )
    .await;

    let logs = drain_captured_config_logs();
    for (_, message) in &logs {
        assert!(
            !message.contains(secret),
            "sensitive api_key leaked into log message: {message}"
        );
    }
}
