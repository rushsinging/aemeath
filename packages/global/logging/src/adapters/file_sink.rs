//! Unified diagnostic logger with independently recoverable file sinks.

use super::async_sink::AsyncSinkWorker;
use super::formatter::format_diag_json_line;
use super::lifecycle::{EmergencyWriter, FileSinkLifecycle, StdFileOps, StdMonotonicClock};
use super::native_stderr::route_native_stderr;
use crate::domain::{DiagnosticSinkId, LoggingOutputMode, LoggingSettings, TargetCatalog};
use log::{Log, Metadata, Record};
use std::collections::HashMap;
use std::io::{self, stderr, BufWriter, Stderr, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const UNKNOWN_TARGET_REPORT_LIMIT: usize = 3;
static UNKNOWN_TARGET_REPORTS: AtomicUsize = AtomicUsize::new(0);

/// 异步落盘 channel 容量：按平均 512B/行计约 4MB 内存上限，
/// 远高于日志峰值速率，饱和时丢弃计数而非反压调用线程。
const ASYNC_SINK_CHANNEL_CAPACITY: usize = 8192;

/// emergency 兜底专用的日志文件名。TUI（alternate screen）下 stderr 越过双缓冲直接糊屏，
/// 因此 File 模式的兜底 **NEVER** 走 stderr，统一落到 `<logs_dir>/emergency.log`。
const EMERGENCY_LOG_FILE: &str = "emergency.log";

struct SinkEntry {
    #[cfg_attr(not(test), allow(dead_code))]
    path: PathBuf,
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

/// 把 emergency 兜底写入 `<logs_dir>/emergency.log` 的 writer。
///
/// 设计目标：TUI alternate screen 下 stderr 会越过 ratatui 双缓冲直接糊屏（见 #1215），
/// 因此 File 模式（含 TUI）的兜底 **NEVER** 走 stderr。打开失败时 best-effort 静默丢弃，
/// **绝不**回退 stderr——宁可丢一行兜底日志，也不污染用户屏幕。
struct FileEmergency {
    path: PathBuf,
}

impl FileEmergency {
    fn new(logs_dir: PathBuf) -> Self {
        Self {
            path: logs_dir.join(EMERGENCY_LOG_FILE),
        }
    }
}

impl EmergencyWriter for FileEmergency {
    fn write(&self, message: &str) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(mut file) => {
                let _ = writeln!(file, "{message}");
                let _ = file.flush();
            }
            Err(_) => { /* 静默，绝不回退 stderr */ }
        }
    }
}

/// The process-wide logger. File 模式下全部落盘 IO 由专用 worker 线程执行，
/// 调用线程（含 TUI 主线程）只做有界入队，磁盘繁忙不再阻塞 UI。
pub struct UnifiedLogger {
    #[cfg_attr(not(test), allow(dead_code))]
    sinks: HashMap<DiagnosticSinkId, SinkEntry>,
    async_sink: Option<AsyncSinkWorker>,
    emergency: Arc<dyn EmergencyWriter>,
    output_mode: LoggingOutputMode,
    filter: env_logger::Logger,
}

static LOGGER: OnceLock<&'static UnifiedLogger> = OnceLock::new();

impl UnifiedLogger {
    /// Installs the global logger. Failure to open one sink degrades only that sink.
    pub fn init(settings: LoggingSettings) -> io::Result<()> {
        route_native_stderr(&settings)?;
        // emergency 兜底按 output_mode 选择：File 模式（含 TUI）落 emergency.log，
        // 避免 stderr 越过 alternate screen 糊屏（#1215）；Stderr 模式（no-tui -v）
        // 保留实时 stderr 语义。
        let emergency: Arc<dyn EmergencyWriter> = match settings.output_mode() {
            LoggingOutputMode::File => {
                Arc::new(FileEmergency::new(settings.logs_dir().to_path_buf()))
            }
            LoggingOutputMode::Stderr => Arc::new(DirectStderr::new()),
        };
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
        let mut lifecycles = HashMap::new();
        let mut add = |sink: DiagnosticSinkId,
                       file_name: &str,
                       lifecycles: &mut HashMap<DiagnosticSinkId, FileSinkLifecycle>|
         -> io::Result<()> {
            let path = settings.logs_dir().join(file_name);
            if settings.output_mode() == LoggingOutputMode::File {
                let lifecycle = FileSinkLifecycle::start(
                    path.clone(),
                    settings.max_bytes(),
                    settings.max_backups(),
                    settings.retention_days(),
                    files.clone(),
                    clock.clone(),
                    emergency.clone(),
                );
                lifecycles.insert(sink, lifecycle);
            }
            insert_sink(&mut sinks, sink, SinkEntry { path })
        };
        let fallback = TargetCatalog::fallback();
        add(fallback.sink, fallback.file_name, &mut lifecycles)?;
        for spec in TargetCatalog::specs() {
            add(spec.sink, spec.file_name, &mut lifecycles)?;
        }
        // File 模式：全部 lifecycle 移交专用落盘线程（enqueue/flush barrier 语义
        // 见 `async_sink`）；Stderr 模式保持实时直写，无 worker。
        let async_sink = (settings.output_mode() == LoggingOutputMode::File).then(|| {
            AsyncSinkWorker::spawn(lifecycles, emergency.clone(), ASYNC_SINK_CHANNEL_CAPACITY)
        });
        Ok(Self {
            sinks,
            async_sink,
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

    fn route_sink_id(&self, target: &str) -> DiagnosticSinkId {
        let spec = TargetCatalog::route(target).unwrap_or_else(|| {
            self.report_unknown_target(target);
            TargetCatalog::fallback()
        });
        spec.sink
    }

    /// 测试辅助：按 target 查 sink 落盘路径（生产日志路径走 `route_sink_id`）。
    #[cfg_attr(not(test), allow(dead_code))]
    fn route(&self, target: &str) -> &SinkEntry {
        let sink = self.route_sink_id(target);
        self.sinks
            .get(&sink)
            .expect("catalog sink must be installed")
    }

    /// 未知 target 报告：写入 fallback sink（aemeath.log），**NEVER** 写 stderr。
    /// 写 stderr 会污染 TUI 屏幕（alternatescreen 下 stderr 直接覆盖渲染区）。
    /// 节流后仍只报告有限次数，避免日志膨胀。
    fn report_unknown_target(&self, target: &str) {
        if should_report_unknown(&UNKNOWN_TARGET_REPORTS) {
            // 异步入队到 fallback sink（aemeath.log），不写 emergency stderr
            self.enqueue_line(
                TargetCatalog::fallback().sink,
                format!("aemeath logging fallback: unknown target {target:?}; using aemeath.log"),
            );
        }
    }

    /// File 模式经异步 worker 落盘；Stderr 模式实时直写（保持 no-tui -v 语义）。
    fn enqueue_line(&self, sink: DiagnosticSinkId, line: String) {
        if self.output_mode == LoggingOutputMode::Stderr {
            self.emergency.write(&line);
            return;
        }
        match &self.async_sink {
            Some(worker) => worker.handle().enqueue_line(sink, line),
            None => self.emergency.write(&line),
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
            let sink = self.route_sink_id(record.target());
            self.enqueue_line(sink, line);
        }
    }

    fn flush(&self) {
        if self.output_mode == LoggingOutputMode::Stderr {
            return;
        }
        if let Some(worker) = &self.async_sink {
            worker.handle().flush_barrier();
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
