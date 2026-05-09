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
use std::io::{self, Write};
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
}

impl LogFile {
    pub fn file_name(self) -> &'static str {
        match self {
            LogFile::Aemeath => "aemeath.log",
            LogFile::Agent => "agent.log",
            LogFile::Panic => "panic.log",
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
        ];
        assert_eq!(names.len(), 3);
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
}
