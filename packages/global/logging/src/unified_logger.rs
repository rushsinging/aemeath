//! UnifiedLogger — 统一日志入口（feature #79 路径 C）。
//!
//! ## 路由
//!
//! 唯一 logger 实现 `log::Log` trait，`log::log!` 宏按 `record.target()` 前缀路由：
//!
//! | target 前缀   | 路由目标       |
//! |---------------|---------------|
//! | `cli::*`      | `tui.log`     |
//! | `hook::*`     | `hook.log`    |
//! | `runtime::*`  | `runtime.log` |
//! | `provider::*` | `provider.log`|
//! | `tools::*`    | `tools.log`   |
//! | `prompt::*`   | `prompt.log`  |
//! | 其他           | `aemeath.log` |
//!
//! 审计日志通过静态方法 `log_input` / `log_output` / `log_user_input` / `audit` 直接写入
//! `input.log` / `output.log` / `audit.log`，绕过 `log::*!` 宏以保留 `serde_json::Value` 原始结构。
//!
//! ## 过滤
//!
//! - `enabled()` 委托 `env_logger::Logger::enabled()`：保留 `RUST_LOG` + `config.level` 解析。
//! - `log_input` / `log_output` / `log_user_input` 额外受 `role_logs_enabled` 控制；`audit()` 始终写入。
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

/// 10 个 sink 的文件路径（用于轮转时重开）
#[derive(Debug, Clone)]
struct SinkPaths {
    aemeath: PathBuf,
    runtime: PathBuf,
    provider: PathBuf,
    tools: PathBuf,
    prompt: PathBuf,
    tui: PathBuf,
    hook: PathBuf,
    input: PathBuf,
    output: PathBuf,
    audit: PathBuf,
}

impl SinkPaths {
    fn from_logs_dir(logs_dir: &Path) -> Self {
        Self {
            aemeath: logs_dir.join("aemeath.log"),
            runtime: logs_dir.join("runtime.log"),
            provider: logs_dir.join("provider.log"),
            tools: logs_dir.join("tools.log"),
            prompt: logs_dir.join("prompt.log"),
            tui: logs_dir.join("tui.log"),
            hook: logs_dir.join("hook.log"),
            input: logs_dir.join("input.log"),
            output: logs_dir.join("output.log"),
            audit: logs_dir.join("audit.log"),
        }
    }
}

/// 统一 logger。
///
/// 通过 `Box::leak` 获得 `'static` 引用并 `log::set_logger`，因此静态方法
/// (`log_input` / `log_output` / `log_user_input` / `audit`) 与 `log::log!` 宏调用均能命中同一实例。
pub struct UnifiedLogger {
    aemeath: Mutex<Option<BufWriter<File>>>,
    runtime: Mutex<Option<BufWriter<File>>>,
    provider: Mutex<Option<BufWriter<File>>>,
    tools: Mutex<Option<BufWriter<File>>>,
    prompt: Mutex<Option<BufWriter<File>>>,
    tui: Mutex<Option<BufWriter<File>>>,
    hook: Mutex<Option<BufWriter<File>>>,
    input: Mutex<Option<BufWriter<File>>>,
    output: Mutex<Option<BufWriter<File>>>,
    audit: Mutex<Option<BufWriter<File>>>,
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
            &paths.runtime,
            &paths.provider,
            &paths.tools,
            &paths.prompt,
            &paths.tui,
            &paths.hook,
            &paths.input,
            &paths.output,
            &paths.audit,
        ] {
            rotate_if_needed(path, max_bytes, max_backups)?;
        }
        let logger = UnifiedLogger {
            aemeath: Mutex::new(Some(open_buf(&paths.aemeath)?)),
            runtime: Mutex::new(Some(open_buf(&paths.runtime)?)),
            provider: Mutex::new(Some(open_buf(&paths.provider)?)),
            tools: Mutex::new(Some(open_buf(&paths.tools)?)),
            prompt: Mutex::new(Some(open_buf(&paths.prompt)?)),
            tui: Mutex::new(Some(open_buf(&paths.tui)?)),
            hook: Mutex::new(Some(open_buf(&paths.hook)?)),
            input: Mutex::new(Some(open_buf(&paths.input)?)),
            output: Mutex::new(Some(open_buf(&paths.output)?)),
            audit: Mutex::new(Some(open_buf(&paths.audit)?)),
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

    /// 记录用户输入到 `input.log`（type="user_input"）。
    pub fn log_user_input(payload: Value) {
        let Some(logger) = Self::current() else {
            return;
        };
        if !logger.role_logs_enabled {
            return;
        }
        let line = format_audit_json_line("user_input", "default", payload);
        logger.write_audit(&logger.input, &logger.paths.input, &line);
    }

    /// 记录审计事件到 `audit.log`。
    pub fn audit(audit_type: &str, payload: Value) {
        let Some(logger) = Self::current() else {
            return;
        };
        let line = format_audit_json_line(audit_type, "audit", payload);
        logger.write_audit(&logger.audit, &logger.paths.audit, &line);
    }

    /// 按 target 前缀路由到对应的诊断 sink。
    /// 返回 `None` 时走兜底 aemeath sink。
    fn route(&self, target: &str) -> Option<(&Mutex<Option<BufWriter<File>>>, &Path)> {
        if target.starts_with("cli::") {
            Some((&self.tui, &self.paths.tui))
        } else if target.starts_with("hook::") {
            Some((&self.hook, &self.paths.hook))
        } else if target.starts_with("runtime::") {
            Some((&self.runtime, &self.paths.runtime))
        } else if target.starts_with("provider::") {
            Some((&self.provider, &self.paths.provider))
        } else if target.starts_with("tools::") {
            Some((&self.tools, &self.paths.tools))
        } else if target.starts_with("prompt::") {
            Some((&self.prompt, &self.paths.prompt))
        } else {
            None
        }
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
        if let Some((sink, path)) = self.route(target) {
            self.write_diag(sink, path, &line);
        } else {
            self.write_diag(&self.aemeath, &self.paths.aemeath, &line);
        }
    }

    fn flush(&self) {
        for sink in [
            &self.aemeath,
            &self.runtime,
            &self.provider,
            &self.tools,
            &self.prompt,
            &self.tui,
            &self.hook,
            &self.input,
            &self.output,
            &self.audit,
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
#[path = "unified_logger_tests.rs"]
mod unified_logger_tests;
