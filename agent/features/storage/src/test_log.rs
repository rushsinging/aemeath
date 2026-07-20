use std::cell::RefCell;
use std::sync::OnceLock;

use crate::LOG_TARGET;

static INSTALL_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

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

pub(crate) fn begin() {
    let result = INSTALL_RESULT.get_or_init(|| {
        log::set_boxed_logger(Box::new(CapturingLogger))
            .map_err(|error| format!("Storage 测试日志捕获器安装失败：{error}"))?;
        log::set_max_level(log::LevelFilter::Trace);
        Ok(())
    });
    if let Err(error) = result {
        panic!("{error}");
    }
    CAPTURED.with(|cell| cell.borrow_mut().clear());
    CAPTURING.with(|flag| flag.set(true));
}

pub(crate) fn end() {
    CAPTURING.with(|flag| flag.set(false));
}

pub(crate) fn drain() -> Vec<(log::Level, String)> {
    CAPTURED.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}
