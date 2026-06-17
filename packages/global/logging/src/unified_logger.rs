//! UnifiedLogger — 统一日志入口（feature #79 路径 C）。
//!
//! ## 路由
//!
//! 唯一 logger 实现 `log::Log` trait，`log::log!` 宏按 `record.target()` 精确匹配 `aemeath:` 前缀路由：
//!
//! | target（最长前缀匹配）| 路由目标            |
//! |------------------------|---------------------|
//! | `aemeath:tui`          | `tui.log`           |
//! | `aemeath:shared`       | `shared.log`        |
//! | `aemeath:composition`  | `composition.log`   |
//! | `aemeath:agent:provider` | `agent-provider.log` |
//! | `aemeath:agent:runtime` | `agent-runtime.log` |
//! | `aemeath:agent:tools`  | `agent-tools.log`   |
//! | `aemeath:agent:prompt` | `agent-prompt.log`  |
//! | `aemeath:agent:hook`   | `agent-hook.log`    |
//! | `aemeath:agent:storage` | `agent-storage.log` |
//! | `aemeath:agent:project` | `agent-project.log` |
//! | `aemeath:agent:policy` | `agent-policy.log`  |
//! | `aemeath:agent:audit`  | `agent-audit.log`   |
//! | 其他                    | `aemeath.log`（硬兜底）|
//!
//! 审计日志通过静态方法 `log_input` / `log_output` / `log_user_input` 直接写入
//! `agent-provider.log`，绕过 `log::*!` 宏以保留 `serde_json::Value` 原始结构。
//!
//! ## 过滤
//!
//! - `enabled()` 委托 `env_logger::Logger::enabled()`：保留 `RUST_LOG` + `config.level` 解析。
//! - `log_input` / `log_output` / `log_user_input` 额外受 `role_logs_enabled` 控制。
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

/// 合法日志 target 白名单（12 个）。
/// 所有 `log::xxx!` 调用的 target 值必须 ∈ 此列表或以此为前缀。
pub const ALLOWED_TARGETS: &[&str] = &[
    "aemeath:tui",
    "aemeath:shared",
    "aemeath:composition",
    "aemeath:agent:provider",
    "aemeath:agent:runtime",
    "aemeath:agent:tools",
    "aemeath:agent:prompt",
    "aemeath:agent:hook",
    "aemeath:agent:storage",
    "aemeath:agent:project",
    "aemeath:agent:policy",
    "aemeath:agent:audit",
];

/// target → 日志文件名映射。最长前缀匹配。
fn target_to_file(target: &str) -> &str {
    for allowed in ALLOWED_TARGETS {
        if target == *allowed || target.starts_with(&format!("{}:", allowed)) {
            return match *allowed {
                "aemeath:tui" => "tui.log",
                "aemeath:shared" => "shared.log",
                "aemeath:composition" => "composition.log",
                "aemeath:agent:provider" => "agent-provider.log",
                "aemeath:agent:runtime" => "agent-runtime.log",
                "aemeath:agent:tools" => "agent-tools.log",
                "aemeath:agent:prompt" => "agent-prompt.log",
                "aemeath:agent:hook" => "agent-hook.log",
                "aemeath:agent:storage" => "agent-storage.log",
                "aemeath:agent:project" => "agent-project.log",
                "aemeath:agent:policy" => "agent-policy.log",
                "aemeath:agent:audit" => "agent-audit.log",
                _ => "aemeath.log",
            };
        }
    }
    "aemeath.log" // 硬兜底（守卫会拦截，不应到达）
}

/// 12 个 sink 的文件路径（用于轮转时重开）
#[derive(Debug, Clone)]
struct SinkPaths {
    aemeath: PathBuf,
    tui: PathBuf,
    shared: PathBuf,
    composition: PathBuf,
    provider: PathBuf,
    runtime: PathBuf,
    tools: PathBuf,
    prompt: PathBuf,
    hook: PathBuf,
    storage: PathBuf,
    project: PathBuf,
    policy: PathBuf,
    audit: PathBuf,
}

impl SinkPaths {
    fn from_logs_dir(logs_dir: &Path) -> Self {
        Self {
            aemeath: logs_dir.join("aemeath.log"),
            tui: logs_dir.join("tui.log"),
            shared: logs_dir.join("shared.log"),
            composition: logs_dir.join("composition.log"),
            provider: logs_dir.join("agent-provider.log"),
            runtime: logs_dir.join("agent-runtime.log"),
            tools: logs_dir.join("agent-tools.log"),
            prompt: logs_dir.join("agent-prompt.log"),
            hook: logs_dir.join("agent-hook.log"),
            storage: logs_dir.join("agent-storage.log"),
            project: logs_dir.join("agent-project.log"),
            policy: logs_dir.join("agent-policy.log"),
            audit: logs_dir.join("agent-audit.log"),
        }
    }

    /// 按文件名返回对应 sink 的路径引用。
    fn path_for_file(&self, file_name: &str) -> &Path {
        match file_name {
            "tui.log" => &self.tui,
            "shared.log" => &self.shared,
            "composition.log" => &self.composition,
            "agent-provider.log" => &self.provider,
            "agent-runtime.log" => &self.runtime,
            "agent-tools.log" => &self.tools,
            "agent-prompt.log" => &self.prompt,
            "agent-hook.log" => &self.hook,
            "agent-storage.log" => &self.storage,
            "agent-project.log" => &self.project,
            "agent-policy.log" => &self.policy,
            "agent-audit.log" => &self.audit,
            _ => &self.aemeath,
        }
    }
}

/// 统一 logger。
///
/// 通过 `Box::leak` 获得 `'static` 引用并 `log::set_logger`，因此静态方法
/// (`log_input` / `log_output` / `log_user_input`) 与 `log::log!` 宏调用均能命中同一实例。
pub struct UnifiedLogger {
    aemeath: Mutex<Option<BufWriter<File>>>,
    tui: Mutex<Option<BufWriter<File>>>,
    shared: Mutex<Option<BufWriter<File>>>,
    composition: Mutex<Option<BufWriter<File>>>,
    provider: Mutex<Option<BufWriter<File>>>,
    runtime: Mutex<Option<BufWriter<File>>>,
    tools: Mutex<Option<BufWriter<File>>>,
    prompt: Mutex<Option<BufWriter<File>>>,
    hook: Mutex<Option<BufWriter<File>>>,
    storage: Mutex<Option<BufWriter<File>>>,
    project: Mutex<Option<BufWriter<File>>>,
    policy: Mutex<Option<BufWriter<File>>>,
    audit: Mutex<Option<BufWriter<File>>>,
    paths: SinkPaths,
    max_bytes: u64,
    max_backups: usize,
    role_logs_enabled: bool,
    filter: env_logger::Logger,
}

/// 全局 logger 引用（`init` 后填充）。
static LOGGER: OnceLock<&'static UnifiedLogger> = OnceLock::new();

/// 所有 sink 的名称列表，用于迭代。
const ALL_SINK_FILENAMES: &[&str] = &[
    "aemeath.log",
    "tui.log",
    "shared.log",
    "composition.log",
    "agent-provider.log",
    "agent-runtime.log",
    "agent-tools.log",
    "agent-prompt.log",
    "agent-hook.log",
    "agent-storage.log",
    "agent-project.log",
    "agent-policy.log",
    "agent-audit.log",
];

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
        for file_name in ALL_SINK_FILENAMES {
            rotate_if_needed(paths.path_for_file(file_name), max_bytes, max_backups)?;
        }
        let logger = UnifiedLogger {
            aemeath: Mutex::new(Some(open_buf(&paths.aemeath)?)),
            tui: Mutex::new(Some(open_buf(&paths.tui)?)),
            shared: Mutex::new(Some(open_buf(&paths.shared)?)),
            composition: Mutex::new(Some(open_buf(&paths.composition)?)),
            provider: Mutex::new(Some(open_buf(&paths.provider)?)),
            runtime: Mutex::new(Some(open_buf(&paths.runtime)?)),
            tools: Mutex::new(Some(open_buf(&paths.tools)?)),
            prompt: Mutex::new(Some(open_buf(&paths.prompt)?)),
            hook: Mutex::new(Some(open_buf(&paths.hook)?)),
            storage: Mutex::new(Some(open_buf(&paths.storage)?)),
            project: Mutex::new(Some(open_buf(&paths.project)?)),
            policy: Mutex::new(Some(open_buf(&paths.policy)?)),
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

    /// 记录 LLM 输入到 `agent-provider.log`。
    pub fn log_input(_role: &str, payload: Value) {
        let Some(logger) = Self::current() else {
            return;
        };
        if !logger.role_logs_enabled {
            return;
        }
        let line = format_audit_json_line("llm_input", payload);
        logger.write_line(&logger.provider, &logger.paths.provider, &line);
    }

    /// 记录 LLM 输出到 `agent-provider.log`。
    pub fn log_output(_role: &str, payload: Value) {
        let Some(logger) = Self::current() else {
            return;
        };
        if !logger.role_logs_enabled {
            return;
        }
        let line = format_audit_json_line("llm_output", payload);
        logger.write_line(&logger.provider, &logger.paths.provider, &line);
    }

    /// 记录用户输入到 `agent-provider.log`（event_type="user_input"）。
    pub fn log_user_input(payload: Value) {
        let Some(logger) = Self::current() else {
            return;
        };
        if !logger.role_logs_enabled {
            return;
        }
        let line = format_audit_json_line("user_input", payload);
        logger.write_line(&logger.provider, &logger.paths.provider, &line);
    }

    /// 按 target 查找对应的诊断 sink。
    /// 返回 `(sink, path)` 元组。
    fn route(&self, target: &str) -> (&Mutex<Option<BufWriter<File>>>, &Path) {
        let file_name = target_to_file(target);
        match file_name {
            "tui.log" => (&self.tui, &self.paths.tui),
            "shared.log" => (&self.shared, &self.paths.shared),
            "composition.log" => (&self.composition, &self.paths.composition),
            "agent-provider.log" => (&self.provider, &self.paths.provider),
            "agent-runtime.log" => (&self.runtime, &self.paths.runtime),
            "agent-tools.log" => (&self.tools, &self.paths.tools),
            "agent-prompt.log" => (&self.prompt, &self.paths.prompt),
            "agent-hook.log" => (&self.hook, &self.paths.hook),
            "agent-storage.log" => (&self.storage, &self.paths.storage),
            "agent-project.log" => (&self.project, &self.paths.project),
            "agent-policy.log" => (&self.policy, &self.paths.policy),
            "agent-audit.log" => (&self.audit, &self.paths.audit),
            _ => (&self.aemeath, &self.paths.aemeath),
        }
    }

    fn write_line(&self, sink: &Mutex<Option<BufWriter<File>>>, path: &Path, line: &str) {
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
        for file_name in ALL_SINK_FILENAMES {
            let (sink, _) = self.route_sink_by_file(file_name);
            if let Ok(mut guard) = sink.lock() {
                if let Some(w) = guard.as_mut() {
                    let _ = w.flush();
                }
            }
        }
    }
}

impl UnifiedLogger {
    /// 按文件名返回对应 sink（用于 flush 遍历）。
    fn route_sink_by_file(&self, file_name: &str) -> (&Mutex<Option<BufWriter<File>>>, &Path) {
        match file_name {
            "tui.log" => (&self.tui, &self.paths.tui),
            "shared.log" => (&self.shared, &self.paths.shared),
            "composition.log" => (&self.composition, &self.paths.composition),
            "agent-provider.log" => (&self.provider, &self.paths.provider),
            "agent-runtime.log" => (&self.runtime, &self.paths.runtime),
            "agent-tools.log" => (&self.tools, &self.paths.tools),
            "agent-prompt.log" => (&self.prompt, &self.paths.prompt),
            "agent-hook.log" => (&self.hook, &self.paths.hook),
            "agent-storage.log" => (&self.storage, &self.paths.storage),
            "agent-project.log" => (&self.project, &self.paths.project),
            "agent-policy.log" => (&self.policy, &self.paths.policy),
            "agent-audit.log" => (&self.audit, &self.paths.audit),
            _ => (&self.aemeath, &self.paths.aemeath),
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
