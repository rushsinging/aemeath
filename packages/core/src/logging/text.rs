use crate::config::paths;
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use super::rotation::{prepare_log_path, timestamp_rfc3339};

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
    paths::global_logs_dir()
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
