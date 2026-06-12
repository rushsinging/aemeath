//! UnifiedLogger — 统一日志入口（feature #79 路径 C）。
//!
//! ## 路由
//!
//! 唯一 logger 实现 `log::Log` trait，`log::log!` 宏按 `record.target()` 前缀路由：
//!
//! | target 前缀 | 路由目标 |
//! |-------------|----------|
//! | `cli::*`    | `tui.log` |
//! | `hook::*`   | `hook.log` |
//! | 其他         | `aemeath.log` |
//!
//! 审计日志通过静态方法 `log_input` / `log_output` / `log_tool` 直接写入
//! `input.log` / `output.log` / `tool.log`，绕过 `log::*!` 宏以保留 `serde_json::Value` 原始结构。
//!
//! ## 过滤
//!
//! - `enabled()` 委托 `env_logger::Logger::enabled()`：保留 `RUST_LOG` + `config.level` 解析。
//! - 审计 API 额外受 `role_logs_enabled` 控制。
//!
//! ## 输出格式
//!
//! 诊断 + 审计均走 **compact JSON Lines**（一行一个 JSON 对象，无 pretty-print 缩进）。
//! 消费者可用 `grep -E '^\{' *.log | jq` 统一处理。

use crate::format::{format_audit_json_line, format_diag_json_line};
use crate::rotation::rotate_if_needed;
use log::{LevelFilter, Log, Metadata, Record};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// tool 审计类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Call,
    Result,
}

impl ToolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolKind::Call => "tool_call",
            ToolKind::Result => "tool_result",
        }
    }
}

/// 6 个 sink 的文件路径（用于轮转时重开）
#[derive(Debug, Clone)]
struct SinkPaths {
    aemeath: PathBuf,
    tui: PathBuf,
    hook: PathBuf,
    input: PathBuf,
    output: PathBuf,
    tool: PathBuf,
}

impl SinkPaths {
    fn from_logs_dir(logs_dir: &Path) -> Self {
        Self {
            aemeath: logs_dir.join("aemeath.log"),
            tui: logs_dir.join("tui.log"),
            hook: logs_dir.join("hook.log"),
            input: logs_dir.join("input.log"),
            output: logs_dir.join("output.log"),
            tool: logs_dir.join("tool.log"),
        }
    }
}

/// 统一 logger。
///
/// 通过 `Box::leak` 获得 `'static` 引用并 `log::set_logger`，因此静态方法
/// (`log_input` / `log_output` / `log_tool`) 与 `log::log!` 宏调用均能命中同一实例。
pub struct UnifiedLogger {
    aemeath: Mutex<Option<BufWriter<File>>>,
    tui: Mutex<Option<BufWriter<File>>>,
    hook: Mutex<Option<BufWriter<File>>>,
    input: Mutex<Option<BufWriter<File>>>,
    output: Mutex<Option<BufWriter<File>>>,
    tool: Mutex<Option<BufWriter<File>>>,
    paths: SinkPaths,
    max_bytes: u64,
    max_backups: usize,
    role_logs_enabled: bool,
    filter: env_logger::Logger,
}

/// 全局 logger 引用（`init` 后填充）。
static LOGGER: OnceLock<&'static UnifiedLogger> = OnceLock::new();

impl UnifiedLogger {
    /// 初始化全局 logger。该函数只能调用一次（`log::set_logger` 限制）。
    ///
    /// - `logs_dir`：日志根目录（不存在则创建）
    /// - `max_bytes` / `max_backups`：单文件轮转阈值与保留份数
    /// - `role_logs_enabled`：是否启用审计 API（input/output/tool）
    /// - `max_level`：最大日志级别（通常从 `config.level` 解析得到）
    pub fn init(
        logs_dir: &Path,
        max_bytes: u64,
        max_backups: usize,
        role_logs_enabled: bool,
        max_level: LevelFilter,
    ) -> io::Result<()> {
        fs::create_dir_all(logs_dir)?;
        let paths = SinkPaths::from_logs_dir(logs_dir);
        for path in [
            &paths.aemeath,
            &paths.tui,
            &paths.hook,
            &paths.input,
            &paths.output,
            &paths.tool,
        ] {
            rotate_if_needed(path, max_bytes, max_backups)?;
        }
        let logger = UnifiedLogger {
            aemeath: Mutex::new(Some(open_buf(&paths.aemeath)?)),
            tui: Mutex::new(Some(open_buf(&paths.tui)?)),
            hook: Mutex::new(Some(open_buf(&paths.hook)?)),
            input: Mutex::new(Some(open_buf(&paths.input)?)),
            output: Mutex::new(Some(open_buf(&paths.output)?)),
            tool: Mutex::new(Some(open_buf(&paths.tool)?)),
            paths,
            max_bytes,
            max_backups,
            role_logs_enabled,
            filter: build_filter(max_level),
        };
        let leaked: &'static UnifiedLogger = Box::leak(Box::new(logger));
        log::set_logger(leaked).map_err(|e| io::Error::other(e.to_string()))?;
        log::set_max_level(max_level);
        // LOGGER 重复 set 会失败，但 init 只能调用一次，与 log::set_logger 一致
        let _ = LOGGER.set(leaked);
        Ok(())
    }

    /// 取得当前全局 logger（`init` 之后才非空）。
    pub fn current() -> Option<&'static UnifiedLogger> {
        LOGGER.get().copied()
    }

    /// 记录 LLM 输入到 `input.log`。
    pub fn log_input(role: &str, payload: Value) {
        let Some(logger) = Self::current() else {
            return;
        };
        if !logger.role_logs_enabled {
            return;
        }
        let line = format_audit_json_line("input", role, payload);
        logger.write_audit(&logger.input, &logger.paths.input, &line);
    }

    /// 记录 LLM 输出到 `output.log`。
    pub fn log_output(role: &str, payload: Value) {
        let Some(logger) = Self::current() else {
            return;
        };
        if !logger.role_logs_enabled {
            return;
        }
        let line = format_audit_json_line("output", role, payload);
        logger.write_audit(&logger.output, &logger.paths.output, &line);
    }

    /// 记录 tool call / result 到 `tool.log`。
    pub fn log_tool(role: &str, kind: ToolKind, payload: Value) {
        let Some(logger) = Self::current() else {
            return;
        };
        if !logger.role_logs_enabled {
            return;
        }
        let line = format_audit_json_line(kind.as_str(), role, payload);
        logger.write_audit(&logger.tool, &logger.paths.tool, &line);
    }

    fn write_audit(&self, sink: &Mutex<Option<BufWriter<File>>>, path: &Path, line: &str) {
        if let Ok(mut guard) = sink.lock() {
            self.maybe_rotate(path, &mut guard);
            if let Some(writer) = guard.as_mut() {
                let _ = writeln!(writer, "{}", line);
                let _ = writer.flush();
            }
        }
    }

    fn write_diag(&self, sink: &Mutex<Option<BufWriter<File>>>, path: &Path, line: &str) {
        if let Ok(mut guard) = sink.lock() {
            self.maybe_rotate(path, &mut guard);
            if let Some(writer) = guard.as_mut() {
                let _ = writeln!(writer, "{}", line);
                let _ = writer.flush();
            }
        }
    }

    /// 在持有 sink 锁（`guard`）的前提下按需轮转。
    ///
    /// **NEVER** 在此重新 `sink.lock()`：调用方 `write_diag` / `write_audit` 已持有该锁，
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
        if target.starts_with("cli::") {
            self.write_diag(&self.tui, &self.paths.tui, &line);
        } else if target.starts_with("hook::") {
            self.write_diag(&self.hook, &self.paths.hook, &line);
        } else {
            self.write_diag(&self.aemeath, &self.paths.aemeath, &line);
        }
    }

    fn flush(&self) {
        for sink in [
            &self.aemeath,
            &self.tui,
            &self.hook,
            &self.input,
            &self.output,
            &self.tool,
        ] {
            if let Ok(mut guard) = sink.lock() {
                if let Some(w) = guard.as_mut() {
                    let _ = w.flush();
                }
            }
        }
    }
}

fn build_filter(max_level: LevelFilter) -> env_logger::Logger {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(max_level);
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        builder.parse_filters(&rust_log);
    }
    builder.build()
}

fn open_buf(path: &Path) -> io::Result<BufWriter<File>> {
    Ok(BufWriter::new(
        OpenOptions::new().create(true).append(true).open(path)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_kind_as_str() {
        assert_eq!(ToolKind::Call.as_str(), "tool_call");
        assert_eq!(ToolKind::Result.as_str(), "tool_result");
    }

    #[test]
    fn sink_paths_in_logs_dir() {
        let paths = SinkPaths::from_logs_dir(Path::new("/tmp/logs"));
        assert_eq!(paths.aemeath, PathBuf::from("/tmp/logs/aemeath.log"));
        assert_eq!(paths.tui, PathBuf::from("/tmp/logs/tui.log"));
        assert_eq!(paths.hook, PathBuf::from("/tmp/logs/hook.log"));
        assert_eq!(paths.input, PathBuf::from("/tmp/logs/input.log"));
        assert_eq!(paths.output, PathBuf::from("/tmp/logs/output.log"));
        assert_eq!(paths.tool, PathBuf::from("/tmp/logs/tool.log"));
    }

    #[test]
    fn static_audit_methods_are_noop_without_init() {
        // 未 init 时 log_input/output/tool 应静默 no-op（不能 panic）
        UnifiedLogger::log_input("default", json!({}));
        UnifiedLogger::log_output("default", json!({}));
        UnifiedLogger::log_tool("default", ToolKind::Call, json!({}));
    }

    /// 构造一个仅用于测试 `maybe_rotate` 的最小 logger（其余 sink 留空）。
    fn rotate_test_logger(dir: &Path, max_bytes: u64, max_backups: usize) -> UnifiedLogger {
        UnifiedLogger {
            aemeath: Mutex::new(None),
            tui: Mutex::new(None),
            hook: Mutex::new(None),
            input: Mutex::new(None),
            output: Mutex::new(None),
            tool: Mutex::new(None),
            paths: SinkPaths::from_logs_dir(dir),
            max_bytes,
            max_backups,
            role_logs_enabled: false,
            filter: build_filter(LevelFilter::Off),
        }
    }

    // 正常路径：达到阈值时轮转，并经 guard 重装新 writer（不重入 sink 锁、不死锁）。
    #[test]
    fn maybe_rotate_rotates_and_reinstalls_writer_when_over_threshold() {
        let dir = std::env::temp_dir().join("aem_rot_over_threshold");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("aemeath.log");
        File::create(&path)
            .unwrap()
            .write_all(&[b'x'; 2048])
            .unwrap();

        let logger = rotate_test_logger(&dir, 1024, 3);
        let sink: Mutex<Option<BufWriter<File>>> = Mutex::new(Some(open_buf(&path).unwrap()));
        {
            // 持锁状态下调用 —— 旧实现会在此对同一把锁重入而自死锁。
            let mut guard = sink.lock().unwrap();
            logger.maybe_rotate(&path, &mut guard);
            assert!(guard.is_some(), "轮转后应经 guard 安装新 writer");
            writeln!(guard.as_mut().unwrap(), "fresh").unwrap();
            guard.as_mut().unwrap().flush().unwrap();
        }
        assert!(dir.join("aemeath.log.1").exists(), "旧内容应轮转到 .1");
        let new_len = fs::metadata(&path).unwrap().len();
        assert!(new_len < 1024, "新文件应只含 fresh 行，实际 {new_len} 字节");
        let _ = fs::remove_dir_all(&dir);
    }

    // 边界：未达阈值时不轮转、不动 writer。
    #[test]
    fn maybe_rotate_is_noop_when_under_threshold() {
        let dir = std::env::temp_dir().join("aem_rot_under_threshold");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("aemeath.log");
        File::create(&path)
            .unwrap()
            .write_all(&[b'x'; 100])
            .unwrap();

        let logger = rotate_test_logger(&dir, 1024, 3);
        let sink: Mutex<Option<BufWriter<File>>> = Mutex::new(Some(open_buf(&path).unwrap()));
        {
            let mut guard = sink.lock().unwrap();
            logger.maybe_rotate(&path, &mut guard);
            assert!(guard.is_some(), "未达阈值不应清空 writer");
        }
        assert!(!dir.join("aemeath.log.1").exists(), "未达阈值不应轮转");
        let _ = fs::remove_dir_all(&dir);
    }

    // 错误路径：目标文件不存在时 metadata 失败，直接 no-op，不创建文件/writer。
    #[test]
    fn maybe_rotate_is_noop_when_file_missing() {
        let dir = std::env::temp_dir().join("aem_rot_missing");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("does_not_exist.log");

        let logger = rotate_test_logger(&dir, 1024, 3);
        let mut guard: Option<BufWriter<File>> = None;
        logger.maybe_rotate(&path, &mut guard);
        assert!(guard.is_none(), "缺文件应直接 no-op");
        assert!(!path.exists(), "no-op 不应创建文件");
        let _ = fs::remove_dir_all(&dir);
    }
}
