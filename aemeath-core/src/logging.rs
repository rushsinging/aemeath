//! 日志文件管理与格式化输出
//!
//! # 日志文件职责
//!
//! | 文件 | 职责 | 内容 |
//! |------|------|------|
//! | `aemeath.log` | **应用主日志**：所有模块的结构化运行日志 | env_logger pipe 接收全部 `log::*` 输出，包含所有 crate（core/cli/llm/tools）的 info/warn/error/debug |
//! | `agent.log` | **Agent 对话审计日志**：LLM 交互的完整记录 | 主 agent 和 sub-agent 的每次 LLM 请求/响应摘要、tool call 触发与结果摘要、token 用量、模型切换。面向"复现对话流程"而非"调试内部状态" |
//! | `panic.log` | **Panic 崩溃日志** | panic 信息 + backtrace |

use chrono::{DateTime, Local};
use serde_json::json;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const LOG_MAX_BACKUPS: usize = 5;
pub const LOG_RETENTION_DAYS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFile {
    Aemeath,
    Agent,
    Panic,
    Input,
    Output,
    Tool,
}

impl LogFile {
    pub fn file_name(self) -> &'static str {
        match self {
            LogFile::Aemeath => "aemeath.log",
            LogFile::Agent => "agent.log",
            LogFile::Panic => "panic.log",
            LogFile::Input => "input.log",
            LogFile::Output => "output.log",
            LogFile::Tool => "tool.log",
        }
    }
}

pub fn log_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".aemeath")
}

/// 根据配置确定日志基础目录：
/// - 有 logs_dir 配置 → 使用配置值
/// - 无配置 → 默认 ~/.aemeath/logs/
pub fn logs_base_dir(logs_dir_config: Option<&str>) -> PathBuf {
    logs_dir_config
        .map(PathBuf::from)
        .unwrap_or_else(|| log_dir().join("logs"))
}

pub fn log_path(log_file: LogFile) -> PathBuf {
    log_dir().join(log_file.file_name())
}

/// 获取指定基础目录下的日志文件路径
pub fn log_path_in_dir(base_dir: &Path, log_file: LogFile) -> PathBuf {
    base_dir.join(log_file.file_name())
}

pub fn prepare_log_file(log_file: LogFile) -> io::Result<PathBuf> {
    let path = log_path(log_file);
    prepare_log_path(&path)?;
    Ok(path)
}

pub fn open_append(log_file: LogFile) -> io::Result<File> {
    let path = prepare_log_file(log_file)?;
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn append_line(log_file: LogFile, line: &str) -> io::Result<()> {
    let mut file = open_append(log_file)?;
    writeln!(file, "{}", line)
}

pub fn format_text_line(session_id: &str, level: &str, module: &str, message: &str) -> String {
    format_text_line_with_turn(session_id, None, level, module, message)
}

pub fn format_text_line_with_turn(
    session_id: &str,
    turn: Option<usize>,
    level: &str,
    module: &str,
    message: &str,
) -> String {
    let turn = turn
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!(
        "[{}] [session:{}] [turn:{}] [{}] [{}] {}",
        timestamp_rfc3339(),
        session_id,
        turn,
        level,
        module,
        message
    )
}

pub fn append_text_line(
    log_file: LogFile,
    session_id: &str,
    level: &str,
    module: &str,
    message: &str,
) -> io::Result<()> {
    append_text_line_with_turn(log_file, session_id, None, level, module, message)
}

pub fn append_text_line_with_turn(
    log_file: LogFile,
    session_id: &str,
    turn: Option<usize>,
    level: &str,
    module: &str,
    message: &str,
) -> io::Result<()> {
    append_line(
        log_file,
        &format_text_line_with_turn(session_id, turn, level, module, message),
    )
}

pub fn append_json_line(
    log_file: LogFile,
    session_id: &str,
    level: &str,
    module: &str,
    message: &str,
    extra: serde_json::Value,
) -> io::Result<()> {
    append_json_line_with_turn(log_file, session_id, None, level, module, message, extra)
}

pub fn append_json_line_with_turn(
    log_file: LogFile,
    session_id: &str,
    turn: Option<usize>,
    level: &str,
    module: &str,
    message: &str,
    extra: serde_json::Value,
) -> io::Result<()> {
    let value = json!({
        "timestamp": timestamp_rfc3339(),
        "session_id": session_id,
        "turn": turn,
        "level": level,
        "module": module,
        "message": message,
        "extra": extra,
    });
    append_line(log_file, &value.to_string())
}

fn timestamp_rfc3339() -> String {
    let now: DateTime<Local> = Local::now();
    now.to_rfc3339()
}

fn prepare_log_path(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        cleanup_old_rotated_logs(parent)?;
    }
    rotate_if_needed(path, LOG_MAX_BYTES, LOG_MAX_BACKUPS)
}

fn rotate_if_needed(path: &Path, max_bytes: u64, max_backups: usize) -> io::Result<()> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    if metadata.len() < max_bytes {
        return Ok(());
    }

    if max_backups == 0 {
        fs::remove_file(path)?;
        return Ok(());
    }

    for index in (1..=max_backups).rev() {
        let from = rotated_path(path, index);
        if !from.exists() {
            continue;
        }
        if index == max_backups {
            fs::remove_file(&from)?;
        } else {
            fs::rename(&from, rotated_path(path, index + 1))?;
        }
    }

    fs::rename(path, rotated_path(path, 1))
}

fn cleanup_old_rotated_logs(dir: &Path) -> io::Result<()> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(LOG_RETENTION_DAYS * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !is_rotated_log_path(&path) {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        if modified < cutoff {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    path.with_file_name(format!("{}.{}", file_name, index))
}

fn is_rotated_log_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some((base, suffix)) = file_name.rsplit_once('.') else {
        return false;
    };
    base.ends_with(".log") && suffix.chars().all(|c| c.is_ascii_digit())
}

// ═══════════════════════════════════════════════════════════════════
// JsonLogger — 分化日志写入器
// ═══════════════════════════════════════════════════════════════════

/// 分化日志写入器。
///
/// 将 LLM 交互日志从 `aemeath.log` 分离为三个独立 JSON 文件：
/// - `input.log`：LLM 输入快照
/// - `output.log`：LLM 完整输出
/// - `tool.log`：工具调用请求 + 结果
pub struct JsonLogger {
    input: BufWriter<File>,
    output: BufWriter<File>,
    tool: BufWriter<File>,
    session_id: String,
    enabled: bool,
}

impl JsonLogger {
    /// 创建 JsonLogger，在 `base_dir` 下打开三个日志文件。
    ///
    /// 目录不存在时自动创建。文件轮转复用 `rotate_if_needed()`。
    /// 如果 `enabled` 为 false，使用 `/dev/null` 占位。
    pub fn new(base_dir: &Path, session_id: String, enabled: bool) -> io::Result<Self> {
        fs::create_dir_all(base_dir)?;
        cleanup_old_rotated_logs(base_dir)?;

        let open = |log_file: LogFile| -> io::Result<BufWriter<File>> {
            let path = log_path_in_dir(base_dir, log_file);
            rotate_if_needed(&path, LOG_MAX_BYTES, LOG_MAX_BACKUPS)?;
            let file = OpenOptions::new().create(true).append(true).open(&path)?;
            Ok(BufWriter::new(file))
        };

        let null = || -> io::Result<BufWriter<File>> {
            let file = OpenOptions::new().write(true).open(if cfg!(windows) {
                "NUL"
            } else {
                "/dev/null"
            })?;
            Ok(BufWriter::new(file))
        };

        let (input, output, tool) = if enabled {
            (
                open(LogFile::Input)?,
                open(LogFile::Output)?,
                open(LogFile::Tool)?,
            )
        } else {
            (null()?, null()?, null()?)
        };

        Ok(Self {
            input,
            output,
            tool,
            session_id,
            enabled,
        })
    }

    /// 写入 input.log — LLM 输入快照
    pub fn log_input(
        &mut self,
        turn: usize,
        role: &str,
        model: &str,
        data: serde_json::Value,
    ) -> io::Result<()> {
        let session_id = self.session_id.clone();
        write_json_line(
            &mut self.input,
            &session_id,
            "input",
            turn,
            role,
            model,
            data,
        )
    }

    /// 写入 output.log — LLM 完整输出
    pub fn log_output(
        &mut self,
        turn: usize,
        role: &str,
        model: &str,
        data: serde_json::Value,
    ) -> io::Result<()> {
        let session_id = self.session_id.clone();
        write_json_line(
            &mut self.output,
            &session_id,
            "output",
            turn,
            role,
            model,
            data,
        )
    }

    /// 写入 tool.log — 工具调用请求
    pub fn log_tool_call(
        &mut self,
        turn: usize,
        role: &str,
        model: &str,
        data: serde_json::Value,
    ) -> io::Result<()> {
        let session_id = self.session_id.clone();
        write_json_line(
            &mut self.tool,
            &session_id,
            "tool_call",
            turn,
            role,
            model,
            data,
        )
    }

    /// 写入 tool.log — 工具调用结果
    pub fn log_tool_result(
        &mut self,
        turn: usize,
        role: &str,
        model: &str,
        data: serde_json::Value,
    ) -> io::Result<()> {
        let session_id = self.session_id.clone();
        write_json_line(
            &mut self.tool,
            &session_id,
            "tool_result",
            turn,
            role,
            model,
            data,
        )
    }

    /// 返回是否已启用
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// 向指定 writer 中写入一行 JSON 日志
fn write_json_line(
    writer: &mut BufWriter<File>,
    session_id: &str,
    log_type: &str,
    turn: usize,
    role: &str,
    model: &str,
    data: serde_json::Value,
) -> io::Result<()> {
    let line = serde_json::json!({
        "ts": timestamp_rfc3339(),
        "session": session_id,
        "turn": turn,
        "role": role,
        "model": model,
        "type": log_type,
        "data": data,
    });
    writeln!(writer, "{}", line)
}

impl Drop for JsonLogger {
    fn drop(&mut self) {
        // BufWriter 自动 flush 到 File
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_file_file_name_happy_path() {
        assert_eq!(LogFile::Aemeath.file_name(), "aemeath.log");
        assert_eq!(LogFile::Agent.file_name(), "agent.log");
    }

    #[test]
    fn test_log_file_file_name_boundary_all_variants() {
        let names = [
            LogFile::Aemeath.file_name(),
            LogFile::Agent.file_name(),
            LogFile::Panic.file_name(),
            LogFile::Input.file_name(),
            LogFile::Output.file_name(),
            LogFile::Tool.file_name(),
        ];
        assert_eq!(names.len(), 6);
        assert!(names.iter().all(|name| name.ends_with(".log")));
    }

    #[test]
    fn test_log_file_new_variants_file_name() {
        assert_eq!(LogFile::Input.file_name(), "input.log");
        assert_eq!(LogFile::Output.file_name(), "output.log");
        assert_eq!(LogFile::Tool.file_name(), "tool.log");
    }

    #[test]
    fn test_logs_base_dir_with_config() {
        let dir = logs_base_dir(Some("/custom/logs"));
        assert_eq!(dir, PathBuf::from("/custom/logs"));
    }

    #[test]
    fn test_logs_base_dir_default() {
        let dir = logs_base_dir(None);
        assert!(dir.ends_with("logs"));
        assert!(dir.starts_with(log_dir()));
    }

    #[test]
    fn test_log_path_in_dir() {
        let base = Path::new("/tmp/test_logs");
        assert_eq!(
            log_path_in_dir(base, LogFile::Input),
            PathBuf::from("/tmp/test_logs/input.log")
        );
        assert_eq!(
            log_path_in_dir(base, LogFile::Output),
            PathBuf::from("/tmp/test_logs/output.log")
        );
        assert_eq!(
            log_path_in_dir(base, LogFile::Tool),
            PathBuf::from("/tmp/test_logs/tool.log")
        );
    }

    #[test]
    fn test_json_logger_new_disabled_does_not_create_files() {
        let tmp =
            std::env::temp_dir().join(format!("aemeath_test_json_logger_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let result = JsonLogger::new(&tmp, "test-session".to_string(), false);
        assert!(result.is_ok());
        // input/output/tool.log 不应存在（写入 /dev/null）
        assert!(!tmp.join("input.log").exists());
        assert!(!tmp.join("output.log").exists());
        assert!(!tmp.join("tool.log").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_json_logger_new_enabled_creates_directory_and_files() {
        let tmp = std::env::temp_dir().join(format!(
            "aemeath_test_json_logger_enabled_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let logger = JsonLogger::new(&tmp, "test-session".to_string(), true);
        assert!(logger.is_ok());
        // 目录和文件应该创建
        assert!(tmp.join("input.log").exists());
        assert!(tmp.join("output.log").exists());
        assert!(tmp.join("tool.log").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_json_logger_write_and_format() {
        let tmp = std::env::temp_dir().join(format!(
            "aemeath_test_json_logger_write_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut logger = JsonLogger::new(&tmp, "session-1".to_string(), true).unwrap();

        logger
            .log_input(3, "default", "gpt-5.5", serde_json::json!({"messages": 5}))
            .unwrap();
        logger
            .log_output(3, "default", "gpt-5.5", serde_json::json!({"tokens": 100}))
            .unwrap();
        logger
            .log_tool_call(3, "default", "gpt-5.5", serde_json::json!({"tool": "Read"}))
            .unwrap();
        logger
            .log_tool_result(3, "default", "gpt-5.5", serde_json::json!({"result": "ok"}))
            .unwrap();
        drop(logger);

        let input_content = std::fs::read_to_string(tmp.join("input.log")).unwrap();
        let output_content = std::fs::read_to_string(tmp.join("output.log")).unwrap();
        let tool_content = std::fs::read_to_string(tmp.join("tool.log")).unwrap();

        // 验证 JSON 格式：取首行，包含必要字段
        let check = |content: &str, expected_type: &str| {
            let line = content.lines().find(|l| !l.trim().is_empty()).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(parsed["session"], "session-1");
            assert_eq!(parsed["turn"], 3);
            assert_eq!(parsed["role"], "default");
            assert_eq!(parsed["model"], "gpt-5.5");
            assert_eq!(parsed["type"], expected_type);
            assert!(!parsed["ts"].as_str().unwrap().is_empty());
        };

        check(&input_content, "input");
        check(&output_content, "output");
        // tool.log 包含 tool_call + tool_result 两行，取最后一行
        let tool_line = tool_content.lines().last().unwrap();
        let tool_parsed: serde_json::Value = serde_json::from_str(tool_line).unwrap();
        assert_eq!(tool_parsed["type"], "tool_result");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_json_logger_disabled_writes_to_null() {
        let tmp = std::env::temp_dir().join(format!(
            "aemeath_test_json_logger_disabled_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut logger = JsonLogger::new(&tmp, "session-1".to_string(), false).unwrap();
        // disabled 时写入不报错
        assert!(logger.log_input(1, "r", "m", serde_json::json!({})).is_ok());
        assert!(logger
            .log_output(1, "r", "m", serde_json::json!({}))
            .is_ok());
        assert!(logger
            .log_tool_call(1, "r", "m", serde_json::json!({}))
            .is_ok());
        assert!(logger
            .log_tool_result(1, "r", "m", serde_json::json!({}))
            .is_ok());
        assert!(!logger.is_enabled());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_format_text_line_happy_path() {
        let line = format_text_line("session-1", "INFO", "agent", "started");
        assert!(line.contains("[session:session-1]"));
        assert!(line.contains("[turn:-]"));
        assert!(line.contains("[INFO]"));
        assert!(line.ends_with("started"));
    }

    #[test]
    fn test_format_text_line_with_turn_happy_path() {
        let line = format_text_line_with_turn("session-1", Some(3), "INFO", "agent", "started");
        assert!(line.contains("[session:session-1]"));
        assert!(line.contains("[turn:3]"));
    }

    #[test]
    fn test_format_text_line_boundary_empty_values() {
        let line = format_text_line("", "", "", "");
        assert!(line.contains("[session:] [turn:-] [] []"));
    }

    #[test]
    fn test_rotated_path_happy_path() {
        let path = PathBuf::from("/tmp/aemeath.log");
        assert_eq!(rotated_path(&path, 2), PathBuf::from("/tmp/aemeath.log.2"));
    }

    #[test]
    fn test_is_rotated_log_path_happy_path() {
        assert!(is_rotated_log_path(Path::new("aemeath.log.1")));
        assert!(is_rotated_log_path(Path::new("agent.log.5")));
    }

    #[test]
    fn test_is_rotated_log_path_error_non_numeric_suffix() {
        assert!(!is_rotated_log_path(Path::new("aemeath.log.old")));
        assert!(!is_rotated_log_path(Path::new("aemeath.log")));
    }
}
