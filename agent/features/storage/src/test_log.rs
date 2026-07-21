use std::cell::RefCell;
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::LOG_TARGET;

static INSTALL_RESULT: OnceLock<Result<(), String>> = OnceLock::new();
static CAPTURE_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct CaptureGuard {
    _lock: MutexGuard<'static, ()>,
    previous_level: log::LevelFilter,
}

thread_local! {
    // 仅捕获调用 begin 的线程；日志断言测试必须使用 current_thread runtime，
    // Storage adapter 也不得在被捕获路径将日志转交给其他线程。
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

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        CAPTURING.with(|flag| flag.set(false));
        log::set_max_level(self.previous_level);
    }
}

pub(crate) fn begin() -> CaptureGuard {
    let lock = CAPTURE_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let result = INSTALL_RESULT.get_or_init(|| {
        log::set_boxed_logger(Box::new(CapturingLogger))
            .map_err(|error| format!("Storage 测试日志捕获器安装失败：{error}"))?;
        Ok(())
    });
    if let Err(error) = result {
        panic!("{error}");
    }
    let previous_level = log::max_level();
    log::set_max_level(log::LevelFilter::Trace);
    CAPTURED.with(|cell| cell.borrow_mut().clear());
    CAPTURING.with(|flag| flag.set(true));
    CaptureGuard {
        _lock: lock,
        previous_level,
    }
}

pub(crate) fn drain() -> Vec<(log::Level, String)> {
    CAPTURED.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}
