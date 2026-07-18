//! UnifiedLogger — 统一日志入口。
//!
//! ## 路由
//!
//! 唯一 logger 实现 `log::Log` trait，并只消费 domain TargetCatalog 执行最长合法前缀路由；
//! target、owner、sink ID 与文件名不在 adapter 重复定义。未知 target 写入 `aemeath.log`
//! 并通过 direct stderr 限频报告。
//!
//! ## 输出模式
//!
//! - `File`（默认）：按 TargetCatalog 路由到独立日志文件。
//! - `Stderr`：所有日志统一输出到 stderr（JSON Lines 格式，`-q` 调试模式）。
//!
//! ## 过滤
//!
//! - `enabled()` 委托 `env_logger::Logger::enabled()`：保留 `AEMEATH_LOG_LEVEL` + `config.level` 解析。
//!
//! ## 输出格式
//!
//! 统一走 **compact JSON Lines**（一行一个 JSON 对象，无 pretty-print 缩进）。
//! 消费者可用 `grep -E '^\{' *.log | jq` 统一处理。

use super::formatter::format_diag_json_line;
use super::lifecycle::rotate_if_needed;
use crate::domain::{DiagnosticSinkId, TargetCatalog};
use log::{LevelFilter, Log, Metadata, Record};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, stderr, BufWriter, Stderr, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

const UNKNOWN_TARGET_REPORT_LIMIT: usize = 3;
static UNKNOWN_TARGET_REPORTS: AtomicUsize = AtomicUsize::new(0);

/// 输出模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// 按 catalog 路由到 17 个日志文件（含兜底）。
    File,
    /// 所有日志统一输出到 stderr（JSON Lines 格式）。
    Stderr,
}

/// 按 catalog 建立的 sink 路径与 writer。
struct SinkEntry {
    path: PathBuf,
    writer: Mutex<Option<BufWriter<File>>>,
}

/// 统一 logger。
///
/// 通过 `Box::leak` 获得 `'static` 引用并 `log::set_logger`，
/// `log::log!` 宏调用均能命中同一实例。
pub struct UnifiedLogger {
    sinks: HashMap<DiagnosticSinkId, SinkEntry>,
    stderr: Mutex<BufWriter<Stderr>>,
    output_mode: OutputMode,
    max_bytes: u64,
    max_backups: usize,
    filter: env_logger::Logger,
}

/// 全局 logger 引用（`init` 后填充）。
static LOGGER: OnceLock<&'static UnifiedLogger> = OnceLock::new();

impl UnifiedLogger {
    /// 初始化全局 logger。该函数只能调用一次（`log::set_logger` 限制）。
    ///
    /// - `logs_dir`：日志根目录（`File` 模式时不存在则创建）
    /// - `max_bytes` / `max_backups`：单文件轮转阈值与保留份数
    /// - `max_level`：最大日志级别（通常从 `config.level` 解析得到）
    /// - `output_mode`：`File` 写文件 / `Stderr` 写 stderr
    pub fn init(
        logs_dir: &Path,
        max_bytes: u64,
        max_backups: usize,
        max_level: LevelFilter,
        output_mode: OutputMode,
    ) -> io::Result<()> {
        if output_mode == OutputMode::File {
            fs::create_dir_all(logs_dir)?;
        }
        let open = |path: PathBuf| -> io::Result<SinkEntry> {
            let writer = if output_mode == OutputMode::File {
                rotate_if_needed(&path, max_bytes, max_backups)?;
                Some(open_buf(&path)?)
            } else {
                None
            };
            Ok(SinkEntry {
                path,
                writer: Mutex::new(writer),
            })
        };
        let mut sinks = HashMap::new();
        let fallback = TargetCatalog::fallback();
        insert_sink(
            &mut sinks,
            fallback.sink,
            open(logs_dir.join(fallback.file_name))?,
        )?;
        for spec in TargetCatalog::specs() {
            insert_sink(&mut sinks, spec.sink, open(logs_dir.join(spec.file_name))?)?;
        }
        let logger = UnifiedLogger {
            sinks,
            stderr: Mutex::new(BufWriter::new(stderr())),
            output_mode,
            max_bytes,
            max_backups,
            filter: build_filter(max_level),
        };
        let leaked: &'static UnifiedLogger = Box::leak(Box::new(logger));
        log::set_logger(leaked).map_err(|e| io::Error::other(e.to_string()))?;
        log::set_max_level(resolve_max_level(max_level));
        // LOGGER 重复 set 会失败，但 init 只能调用一次，与 log::set_logger 一致
        let _ = LOGGER.set(leaked);
        Ok(())
    }

    /// 取得当前全局 logger（`init` 之后才非空）。
    pub fn current() -> Option<&'static UnifiedLogger> {
        LOGGER.get().copied()
    }

    /// 按 target 查找对应的诊断 sink。
    /// 返回 `(sink, path)` 元组。
    fn route(&self, target: &str) -> (&Mutex<Option<BufWriter<File>>>, &Path) {
        let spec = TargetCatalog::route(target).unwrap_or_else(|| {
            self.report_unknown_target(target);
            TargetCatalog::fallback()
        });
        let entry = self
            .sinks
            .get(&spec.sink)
            .expect("catalog sink must be installed");
        (&entry.writer, &entry.path)
    }

    fn report_unknown_target(&self, target: &str) {
        if !should_report_unknown(&UNKNOWN_TARGET_REPORTS) {
            return;
        }
        if let Ok(mut stderr) = self.stderr.lock() {
            let _ = write_unknown_target_report(&mut *stderr, target);
            let _ = stderr.flush();
        }
    }

    fn write_line(&self, sink: &Mutex<Option<BufWriter<File>>>, path: &Path, line: &str) {
        if self.output_mode == OutputMode::Stderr {
            if let Ok(mut w) = self.stderr.lock() {
                let _ = writeln!(w, "{}", line);
                let _ = w.flush();
            }
        } else if let Ok(mut guard) = sink.lock() {
            self.maybe_rotate(path, &mut guard);
            if let Some(writer) = guard.as_mut() {
                let _ = writeln!(writer, "{}", line);
                let _ = writer.flush();
            }
        }
    }

    /// 在持有 sink 锁（`guard`）的前提下按需轮转。
    ///
    /// **NEVER** 在此重新 `sink.lock()`：调用方 `write_line` 已持有该锁，
    /// `std::sync::Mutex` 不可重入，重入会让写日志的线程自死锁。新 writer 直接经
    /// `guard` 安装。
    fn maybe_rotate(&self, path: &Path, guard: &mut Option<BufWriter<File>>) {
        let need_rotate = fs::metadata(path)
            .map(|m| m.len() >= self.max_bytes)
            .unwrap_or(false);
        if !need_rotate {
            return;
        }
        if let Some(mut w) = guard.take() {
            let _ = w.flush();
        }
        let _ = rotate_if_needed(path, self.max_bytes, self.max_backups);
        if let Ok(new) = open_buf(path) {
            *guard = Some(new);
        }
    }
}

impl Log for UnifiedLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.filter.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_diag_json_line(record);
        let target = record.target();
        let (sink, path) = self.route(target);
        self.write_line(sink, path, &line);
    }

    fn flush(&self) {
        if self.output_mode == OutputMode::Stderr {
            if let Ok(mut w) = self.stderr.lock() {
                let _ = w.flush();
            }
        } else {
            for entry in self.sinks.values() {
                if let Ok(mut guard) = entry.writer.lock() {
                    if let Some(w) = guard.as_mut() {
                        let _ = w.flush();
                    }
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

fn write_unknown_target_report(writer: &mut dyn Write, target: &str) -> io::Result<()> {
    writeln!(
        writer,
        "aemeath logging fallback: unknown target {target:?}; using aemeath.log"
    )
}

fn build_filter(config_level: LevelFilter) -> env_logger::Logger {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(config_level);
    if let Ok(aemeath_log) = std::env::var("AEMEATH_LOG_LEVEL") {
        builder.parse_filters(&aemeath_log);
    }
    builder.build()
}

/// Resolve the effective `set_max_level` from `AEMEATH_LOG_LEVEL` directive.
///
/// Scans the directive string for the most permissive level.
/// If `AEMEATH_LOG_LEVEL` is unset, falls back to `config_level`.
pub fn resolve_max_level(config_level: LevelFilter) -> LevelFilter {
    match std::env::var("AEMEATH_LOG_LEVEL") {
        Ok(directive) => parse_max_level(&directive).max(config_level),
        Err(_) => config_level,
    }
}

/// Parse a directive string and return the most permissive level found.
///
/// Examples:
/// - `"info"` → `Info`
/// - `"debug"` → `Debug`
/// - `"aemeath:tui=debug,aemeath:agent:runtime=trace"` → `Trace`
/// - `""` → `LevelFilter::max()` (off filter = allow all)
fn parse_max_level(directive: &str) -> LevelFilter {
    let directive = directive.trim();
    if directive.is_empty() {
        return LevelFilter::max();
    }

    let mut max = LevelFilter::Off;
    for part in directive.split(|c: char| c == ',' || c == '=' || c.is_whitespace()) {
        let level = match part.to_lowercase().as_str() {
            "trace" => LevelFilter::Trace,
            "debug" => LevelFilter::Debug,
            "info" => LevelFilter::Info,
            "warn" => LevelFilter::Warn,
            "error" => LevelFilter::Error,
            "off" => LevelFilter::Off,
            _ => continue,
        };
        if level > max {
            max = level;
        }
    }
    // If no level keyword found at all (e.g. only target names),
    // default to max to avoid blocking logs.
    if max == LevelFilter::Off {
        return LevelFilter::max();
    }
    max
}

fn open_buf(path: &Path) -> io::Result<BufWriter<File>> {
    Ok(BufWriter::new(
        OpenOptions::new().create(true).append(true).open(path)?,
    ))
}

#[cfg(test)]
#[path = "file_sink_tests.rs"]
mod file_sink_tests;
