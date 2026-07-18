use super::*;
use crate::domain::{LoggingOutputMode, LoggingSettings};
use log::LevelFilter;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "aemeath-logging-{}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn settings(
    dir: &Path,
    max_bytes: u64,
    max_backups: usize,
    retention_days: u64,
) -> LoggingSettings {
    LoggingSettings::new(
        "trace".to_string(),
        LoggingOutputMode::File,
        dir.to_path_buf(),
        max_bytes,
        max_backups,
        retention_days,
    )
}

#[test]
fn build_creates_every_catalog_sink_in_its_own_tempdir() {
    let dir = TestDir::new();
    let logger =
        UnifiedLogger::build(settings(&dir.0, 1024, 3, 0), Arc::new(DirectStderr::new())).unwrap();
    assert_eq!(logger.filter.filter(), LevelFilter::Trace);
    for spec in TargetCatalog::specs() {
        let entry = logger.sinks.get(&spec.sink).unwrap();
        assert_eq!(entry.path, dir.0.join(spec.file_name));
        assert!(entry.path.exists());
    }
}

#[test]
fn real_files_rotate_and_reopen() {
    let dir = TestDir::new();
    let active = dir.0.join(TargetCatalog::fallback().file_name);
    fs::write(&active, b"over threshold").unwrap();
    let logger =
        UnifiedLogger::build(settings(&dir.0, 1, 2, 0), Arc::new(DirectStderr::new())).unwrap();
    let entry = logger.sinks.get(&TargetCatalog::fallback().sink).unwrap();
    logger.write_line(entry, "fresh");
    assert!(dir.0.join("aemeath.log.1").exists());
    assert!(fs::read_to_string(active).unwrap().contains("fresh"));
}

#[test]
fn max_backups_zero_discards_active_and_recreates_it() {
    let dir = TestDir::new();
    let active = dir.0.join(TargetCatalog::fallback().file_name);
    fs::write(&active, b"old").unwrap();
    let logger =
        UnifiedLogger::build(settings(&dir.0, 1, 0, 0), Arc::new(DirectStderr::new())).unwrap();
    let entry = logger.sinks.get(&TargetCatalog::fallback().sink).unwrap();
    logger.write_line(entry, "new");
    assert_eq!(fs::read_to_string(active).unwrap(), "new\n");
    assert!(!dir.0.join("aemeath.log.1").exists());
}

#[test]
fn route_uses_fallback_for_unknown_target() {
    let dir = TestDir::new();
    let logger =
        UnifiedLogger::build(settings(&dir.0, 1024, 1, 0), Arc::new(DirectStderr::new())).unwrap();
    assert_eq!(
        logger.route("unknown::module").path,
        dir.0.join("aemeath.log")
    );
}

#[test]
fn unknown_target_reporting_is_limited() {
    let counter = AtomicUsize::new(0);
    assert!(should_report_unknown(&counter));
    assert!(should_report_unknown(&counter));
    assert!(should_report_unknown(&counter));
    assert!(!should_report_unknown(&counter));
}
