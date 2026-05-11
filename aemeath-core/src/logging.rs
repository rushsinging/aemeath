//! 日志文件管理与格式化输出
//!
//! # 日志文件职责
//!
//! | 文件 | 职责 | 内容 |
//! |------|------|------|
//! | `aemeath.log` | **应用主日志**：所有模块的结构化运行日志 | env_logger pipe 接收全部 `log::*` 输出，包含所有 crate（core/cli/llm/tools）的 info/warn/error/debug |
//! | `input.log` | **LLM 输入快照** | 每次 API 调用的新增 messages 摘要、system blocks、tool schemas |
//! | `output.log` | **LLM 完整输出** | 模型返回的完整 content blocks、token 用量、耗时 |
//! | `tool.log` | **工具调用记录** | 工具调用请求参数 + 工具执行结果（完整输出） |
//! | `agent.log` | **已废弃**：Agent 对话审计日志 | 无写入点，保留枚举兼容 |
//! | `panic.log` | **Panic 崩溃日志** | panic 信息 + backtrace |

use chrono::{DateTime, Local};
use serde_json::json;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::config::logging::LoggingConfig;

pub const LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const LOG_MAX_BACKUPS: usize = 5;
pub const LOG_RETENTION_DAYS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFile {
    Aemeath,
    /// 已废弃：无写入点，保留枚举兼容
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

pub fn log_path(log_file: LogFile) -> PathBuf {
    log_dir().join(log_file.file_name())
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

// ---------------------------------------------------------------------------
// JsonLogger — 分化日志（input.log / output.log / tool.log）
// ---------------------------------------------------------------------------

/// 分化日志写入器，将 agent 交互按职责写入三个独立 JSON 日志文件。
///
/// 所有方法为 `&mut self`，调用方应通过 `Arc<Mutex<JsonLogger>>` 共享。
pub struct JsonLogger {
    input: BufWriter<File>,
    output: BufWriter<File>,
    tool: BufWriter<File>,
    input_path: PathBuf,
    output_path: PathBuf,
    tool_path: PathBuf,
    session_id: String,
    config: LoggingConfig,
}

impl JsonLogger {
    /// 创建 JsonLogger，自动创建日志目录并打开三个文件。
    ///
    /// 如果目录不存在则创建。文件以 append + create 模式打开。
    pub fn new(session_id: &str, logs_dir: &Path, config: &LoggingConfig) -> io::Result<Self> {
        fs::create_dir_all(logs_dir)?;

        let input_path = logs_dir.join("input.log");
        let output_path = logs_dir.join("output.log");
        let tool_path = logs_dir.join("tool.log");

        rotate_if_needed(&input_path, config.max_bytes, config.max_backups)?;
        rotate_if_needed(&output_path, config.max_bytes, config.max_backups)?;
        rotate_if_needed(&tool_path, config.max_bytes, config.max_backups)?;

        let input = BufWriter::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&input_path)?,
        );
        let output = BufWriter::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&output_path)?,
        );
        let tool = BufWriter::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&tool_path)?,
        );

        Ok(Self {
            input,
            output,
            tool,
            input_path,
            output_path,
            tool_path,
            session_id: session_id.to_string(),
            config: config.clone(),
        })
    }

    /// 记录 LLM 输入快照到 `input.log`。
    pub fn log_input(
        &mut self,
        turn: usize,
        role: &str,
        model: &str,
        data: serde_json::Value,
    ) -> io::Result<()> {
        let path = self.input_path.clone();
        write_role_entry(
            &mut self.input,
            &path,
            "input",
            turn,
            role,
            model,
            data,
            &self.session_id,
            &self.config,
        )
    }

    /// 记录 LLM 完整输出到 `output.log`。
    pub fn log_output(
        &mut self,
        turn: usize,
        role: &str,
        model: &str,
        data: serde_json::Value,
    ) -> io::Result<()> {
        let path = self.output_path.clone();
        write_role_entry(
            &mut self.output,
            &path,
            "output",
            turn,
            role,
            model,
            data,
            &self.session_id,
            &self.config,
        )
    }

    /// 记录工具调用请求到 `tool.log`。
    pub fn log_tool_call(
        &mut self,
        turn: usize,
        role: &str,
        model: &str,
        data: serde_json::Value,
    ) -> io::Result<()> {
        let path = self.tool_path.clone();
        write_role_entry(
            &mut self.tool,
            &path,
            "tool_call",
            turn,
            role,
            model,
            data,
            &self.session_id,
            &self.config,
        )
    }

    /// 记录工具执行结果到 `tool.log`。
    pub fn log_tool_result(
        &mut self,
        turn: usize,
        role: &str,
        model: &str,
        data: serde_json::Value,
    ) -> io::Result<()> {
        let path = self.tool_path.clone();
        write_role_entry(
            &mut self.tool,
            &path,
            "tool_result",
            turn,
            role,
            model,
            data,
            &self.session_id,
            &self.config,
        )
    }
}

/// 内部统一写入函数
fn write_role_entry(
    writer: &mut BufWriter<File>,
    path: &Path,
    event_type: &str,
    turn: usize,
    role: &str,
    model: &str,
    data: serde_json::Value,
    session_id: &str,
    config: &LoggingConfig,
) -> io::Result<()> {
    check_rotate(writer, path, config)?;

    let entry = json!({
        "ts": timestamp_rfc3339(),
        "session": session_id,
        "turn": turn,
        "role": role,
        "model": model,
        "type": event_type,
        "data": data,
    });
    writeln!(
        writer,
        "{}",
        serde_json::to_string(&entry).unwrap_or_default()
    )?;
    writer.flush()
}

/// 检查文件大小，超过 max_bytes 时轮转并重新打开
fn check_rotate(
    writer: &mut BufWriter<File>,
    path: &Path,
    config: &LoggingConfig,
) -> io::Result<()> {
    let need_rotate = fs::metadata(path)
        .map(|m| m.len() >= config.max_bytes)
        .unwrap_or(false);
    if need_rotate {
        writer.flush()?;
        rotate_if_needed(path, config.max_bytes, config.max_backups)?;
        *writer = BufWriter::new(OpenOptions::new().create(true).append(true).open(path)?);
    }
    Ok(())
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

    #[test]
    fn test_json_logger_log_input_happy_path_writes_user_message() {
        let temp = std::env::temp_dir().join(format!(
            "aemeath-json-logger-test-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp).unwrap();
        let mut logger = JsonLogger::new("session-1", &temp, &LoggingConfig::default()).unwrap();

        logger
            .log_input(
                1,
                "default",
                "model-1",
                json!({"messages":[{"role":"user","content":"hello"}]}),
            )
            .unwrap();

        let content = fs::read_to_string(temp.join("input.log")).unwrap();
        assert!(content.contains("\"session\":\"session-1\""));
        assert!(content.contains("\"type\":\"input\""));
        assert!(content.contains("hello"));
    }
}
