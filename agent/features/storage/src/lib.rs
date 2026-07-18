/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:storage";
mod adapters;
mod domain;
#[cfg(test)]
#[path = "domain_tests.rs"]
mod domain_tests;
mod memory_store;
mod ports;
mod task_store;

pub mod api {
    pub use crate::{
        AtomicBlobPort, AtomicDatasetPort, BlobRead, CommitWarning, DatasetCommitReceipt,
        DatasetCommitVisibility, DatasetKey, DatasetManifest, DatasetMember, DatasetRead,
        DatasetReadOutcome, DatasetRevision, DeleteOptions, DeleteOutcome, Durability, Generation,
        PreviousPolicy, PromoteOutcome, QuarantineOutcome, QuarantineReason, QuarantineReceipt,
        ReadOutcome, SafeOpenOptions, SafePathSegment, SafeStorageDir, SafeStorageEntry,
        SafeStorageFileType, SafeStorageRoot, StorageError, StorageErrorKind, StorageKey,
        StorageNamespace, TransactionScope, WriteOptions, WriteReceipt,
    };

    pub fn file_system_blob(
        root: impl AsRef<std::path::Path>,
    ) -> Result<std::sync::Arc<dyn AtomicBlobPort>, StorageError> {
        log::debug!(target: crate::LOG_TARGET, "file_system_blob init enter");
        match crate::FileSystemBlobAdapter::new(root) {
            Ok(adapter) => {
                log::info!(target: crate::LOG_TARGET, "file_system_blob init ok");
                Ok(std::sync::Arc::new(adapter))
            }
            Err(error) => {
                log::error!(target: crate::LOG_TARGET, "file_system_blob init failed");
                Err(error)
            }
        }
    }
}

pub use adapters::{
    FileSystemBlobAdapter, FileSystemDatasetAdapter, SafeOpenOptions, SafeStorageDir,
    SafeStorageEntry, SafeStorageFileType, SafeStorageRoot,
};
pub use domain::{
    decide_blob_recovery, decide_orphan_previous, BlobRead, CommitWarning, CorruptTransactionError,
    CorruptionReason, DatasetCommitReceipt, DatasetCommitVisibility, DatasetKey, DatasetManifest,
    DatasetMember, DatasetRead, DatasetReadOutcome, DatasetRevision, DeleteOptions, DeleteOutcome,
    DigestObservation, Durability, Generation, JournalPhase, PreviousPolicy, PromoteOutcome,
    QuarantineDisposition, QuarantineOutcome, QuarantineReason, QuarantineReceipt, ReadOutcome,
    RecoveryDecision, SafePathSegment, StorageError, StorageErrorKind, StorageKey,
    StorageNamespace, TransactionDigest, TransactionScope, WriteOptions, WriteReceipt,
};
pub use memory_store::{
    memory_base_dir, project_file_name, project_file_name_from_path, MemoryStore,
};
pub use ports::{AtomicBlobPort, AtomicDatasetPort};
pub use task_store::{
    Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus, TaskStore, TaskStoreStats,
};

// ---------------------------------------------------------------------------
// #[cfg(test)] 线程局部日志捕获器
// ---------------------------------------------------------------------------

/// 一个极简的、仅测试用的 `log` 后端：安装一次后，通过线程局部开关按线程
/// 独立捕获 `aemeath:agent:storage` target 的日志记录。多个 test 模块共享此
/// 设施——调用 `begin()` 开始捕获、`end()` 停止、`drain()` 取走已捕获的记录。
#[cfg(test)]
pub(crate) mod test_log {
    use std::cell::RefCell;
    use std::sync::Once;

    use crate::LOG_TARGET;

    static INIT: Once = Once::new();

    thread_local! {
        static CAPTURED: RefCell<Vec<(log::Level, String)>> = const { RefCell::new(Vec::new()) };
        static CAPTURING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    struct CapturingLogger;

    impl log::Log for CapturingLogger {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.target().starts_with(LOG_TARGET)
        }

        fn log(&self, record: &log::Record) {
            if !self.enabled(record.metadata()) {
                return;
            }
            CAPTURING.with(|flag| {
                if flag.get() {
                    CAPTURED.with(|cell| {
                        cell.borrow_mut()
                            .push((record.level(), record.args().to_string()));
                    });
                }
            });
        }

        fn flush(&self) {}
    }

    /// 安装全局捕获 logger（仅一次），清空当前线程缓冲并开启捕获。
    pub(crate) fn begin() {
        INIT.call_once(|| {
            let _ = log::set_boxed_logger(Box::new(CapturingLogger));
            log::set_max_level(log::LevelFilter::Trace);
        });
        CAPTURED.with(|cell| cell.borrow_mut().clear());
        CAPTURING.with(|flag| flag.set(true));
    }

    /// 关闭当前线程的捕获。
    pub(crate) fn end() {
        CAPTURING.with(|flag| flag.set(false));
    }

    /// 取走当前线程已捕获的 (level, message) 列表。
    pub(crate) fn drain() -> Vec<(log::Level, String)> {
        CAPTURED.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
    }
}

// ---------------------------------------------------------------------------
// #[cfg(test)] file_system_blob 构造日志 TDD
// ---------------------------------------------------------------------------

#[cfg(test)]
mod file_system_blob_logging_tests {
    use super::api;
    use super::test_log;

    /// 成功路径：file_system_blob 必须先 emit "enter"，再 emit "ok"。
    #[test]
    fn init_success_emits_enter_then_ok() {
        let dir = tempfile::tempdir().expect("temp dir");
        test_log::begin();
        let result = api::file_system_blob(dir.path());
        test_log::end();

        assert!(result.is_ok(), "construction should succeed");
        let logs = test_log::drain();
        let has_enter = logs.iter().any(|(_, m)| m.contains("enter"));
        let has_ok = logs
            .iter()
            .any(|(level, m)| *level <= log::Level::Info && m.contains("ok"));
        assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
        assert!(
            has_ok,
            "expected an 'ok' log line at Info-or-lower, got {logs:?}"
        );
    }

    /// 失败路径：file_system_blob 必须先 emit "enter"，再以 Error 级别 emit "failed"。
    #[test]
    fn init_failure_emits_enter_then_failed_at_error() {
        // 传一个「父路径是文件」的路径，create_dir_all 必然失败。
        let file = tempfile::NamedTempFile::new().expect("temp file");
        test_log::begin();
        let result = api::file_system_blob(file.path().join("subdir"));
        test_log::end();

        assert!(result.is_err(), "construction should fail");
        let logs = test_log::drain();
        let has_enter = logs.iter().any(|(_, m)| m.contains("enter"));
        let has_failed = logs
            .iter()
            .any(|(level, m)| *level == log::Level::Error && m.contains("failed"));
        assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
        assert!(
            has_failed,
            "expected a 'failed' log line at Error level, got {logs:?}"
        );
    }

    /// 日志不得泄露完整文件系统路径。
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
}
