//! Unified diagnostic logger with independently recoverable file sinks.

use super::formatter::format_diag_json_line;
use super::lifecycle::{EmergencyWriter, FileSinkLifecycle, StdFileOps, StdMonotonicClock};
use crate::domain::{DiagnosticSinkId, LoggingOutputMode, LoggingSettings, TargetCatalog};
use log::{Log, Metadata, Record};
use std::collections::HashMap;
use std::io::{self, stderr, BufWriter, Stderr, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const UNKNOWN_TARGET_REPORT_LIMIT: usize = 3;
static UNKNOWN_TARGET_REPORTS: AtomicUsize = AtomicUsize::new(0);

struct SinkEntry {
    #[cfg_attr(not(test), allow(dead_code))]
    path: PathBuf,
    lifecycle: Mutex<Option<FileSinkLifecycle>>,
}

struct DirectStderr {
    writer: Mutex<BufWriter<Stderr>>,
}

impl DirectStderr {
    fn new() -> Self {
        Self {
            writer: Mutex::new(BufWriter::new(stderr())),
        }
    }
}

impl EmergencyWriter for DirectStderr {
    fn write(&self, message: &str) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writeln!(writer, "{message}");
            let _ = writer.flush();
        }
    }
}

/// The process-wide logger. Each file sink owns a separate lifecycle mutex.
pub struct UnifiedLogger {
    sinks: HashMap<DiagnosticSinkId, SinkEntry>,
    emergency: Arc<dyn EmergencyWriter>,
    output_mode: LoggingOutputMode,
    filter: env_logger::Logger,
}

static LOGGER: OnceLock<&'static UnifiedLogger> = OnceLock::new();

impl UnifiedLogger {
    /// Installs the global logger. Failure to open one sink degrades only that sink.
    pub fn init(settings: LoggingSettings) -> io::Result<()> {
        let emergency: Arc<dyn EmergencyWriter> = Arc::new(DirectStderr::new());
        let logger = Self::build(settings, emergency)?;
        let max_level = logger.filter.filter();
        let leaked: &'static UnifiedLogger = Box::leak(Box::new(logger));
        log::set_logger(leaked).map_err(|error| io::Error::other(error.to_string()))?;
        log::set_max_level(max_level);
        let _ = LOGGER.set(leaked);
        Ok(())
    }

    fn build(settings: LoggingSettings, emergency: Arc<dyn EmergencyWriter>) -> io::Result<Self> {
        if settings.output_mode() == LoggingOutputMode::File {
            std::fs::create_dir_all(settings.logs_dir())?;
        }
        let files = Arc::new(StdFileOps);
        let clock = Arc::new(StdMonotonicClock::default());
        let mut sinks = HashMap::new();
        let mut add = |sink: DiagnosticSinkId, file_name: &str| -> io::Result<()> {
            let path = settings.logs_dir().join(file_name);
            let lifecycle = (settings.output_mode() == LoggingOutputMode::File).then(|| {
                FileSinkLifecycle::start(
                    path.clone(),
                    settings.max_bytes(),
                    settings.max_backups(),
                    settings.retention_days(),
                    files.clone(),
                    clock.clone(),
                    emergency.clone(),
                )
            });
            insert_sink(
                &mut sinks,
                sink,
                SinkEntry {
                    path,
                    lifecycle: Mutex::new(lifecycle),
                },
            )
        };
        let fallback = TargetCatalog::fallback();
        add(fallback.sink, fallback.file_name)?;
        for spec in TargetCatalog::specs() {
            add(spec.sink, spec.file_name)?;
        }
        Ok(Self {
            sinks,
            emergency,
            output_mode: settings.output_mode(),
            filter: build_filter(settings.filter_directive()),
        })
    }

    pub fn current() -> Option<&'static UnifiedLogger> {
        LOGGER.get().copied()
    }

    /// Returns the immutable process-wide output mode selected at initialization.
    pub fn output_mode(&self) -> LoggingOutputMode {
        self.output_mode
    }

    fn route(&self, target: &str) -> &SinkEntry {
        let spec = TargetCatalog::route(target).unwrap_or_else(|| {
            self.report_unknown_target(target);
            TargetCatalog::fallback()
        });
        self.sinks
            .get(&spec.sink)
            .expect("catalog sink must be installed")
    }

    /// 未知 target 报告：写入 fallback sink（aemeath.log），**NEVER** 写 stderr。
    /// 写 stderr 会污染 TUI 屏幕（alternatescreen 下 stderr 直接覆盖渲染区）。
    /// 节流后仍只报告有限次数，避免日志膨胀。
    fn report_unknown_target(&self, target: &str) {
        if should_report_unknown(&UNKNOWN_TARGET_REPORTS) {
            // 写入 fallback sink（aemeath.log），不写 emergency stderr
            let fallback = TargetCatalog::fallback();
            if let Some(entry) = self.sinks.get(&fallback.sink) {
                match entry.lifecycle.lock() {
                    Ok(mut lifecycle) => {
                        if let Some(lifecycle) = lifecycle.as_mut() {
                            lifecycle.write_line(&format!(
                                "aemeath logging fallback: unknown target {target:?}; using aemeath.log"
                            ));
                        }
                    }
                    Err(_) => { /* sink 锁失败时静默，不退回 stderr */ }
                }
            }
        }
    }

    fn write_line(&self, entry: &SinkEntry, line: &str) {
        if self.output_mode == LoggingOutputMode::Stderr {
            self.emergency.write(line);
            return;
        }
        match entry.lifecycle.lock() {
            Ok(mut lifecycle) => {
                if let Some(lifecycle) = lifecycle.as_mut() {
                    lifecycle.write_line(line);
                }
            }
            Err(_) => self.emergency.write(line),
        }
    }
}

impl Log for UnifiedLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.filter.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let line = format_diag_json_line(record);
            self.write_line(self.route(record.target()), &line);
        }
    }

    fn flush(&self) {
        if self.output_mode == LoggingOutputMode::Stderr {
            return;
        }
        for entry in self.sinks.values() {
            if let Ok(mut lifecycle) = entry.lifecycle.lock() {
                if let Some(lifecycle) = lifecycle.as_mut() {
                    lifecycle.flush();
                }
            }
        }
    }
}

fn insert_sink(
    sinks: &mut HashMap<DiagnosticSinkId, SinkEntry>,
    sink: DiagnosticSinkId,
    entry: SinkEntry,
) -> io::Result<()> {
    if sinks.insert(sink, entry).is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("duplicate diagnostic sink id: {sink:?}"),
        ));
    }
    Ok(())
}

fn should_report_unknown(counter: &AtomicUsize) -> bool {
    counter.fetch_add(1, Ordering::Relaxed) < UNKNOWN_TARGET_REPORT_LIMIT
}

fn build_filter(directive: &str) -> env_logger::Logger {
    let mut builder = env_logger::Builder::new();
    builder.parse_filters(directive);
    builder.build()
}

#[cfg(test)]
#[path = "file_sink_tests.rs"]
mod file_sink_tests;

#[cfg(test)]
#[path = "file_sink_fault_tests.rs"]
mod file_sink_fault_tests;
