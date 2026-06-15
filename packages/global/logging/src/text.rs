//! 纯文本 + JSON 行日志写入。
//!
//! 所有写文件函数通过 `base_dir: &Path` 参数传入日志根目录，
//! 不再硬编码全局路径。

use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::rotation::{prepare_log_path, timestamp_rfc3339};

pub enum LogFile {
    Aemeath,
    Runtime,
    Provider,
    Tools,
    Prompt,
    Panic,
    Input,
    Output,
    Audit,
    /// 已废弃：无写入点，保留枚举兼容
    Agent,
}

impl LogFile {
    pub fn file_name(self) -> &'static str {
        match self {
            LogFile::Aemeath => "aemeath.log",
            LogFile::Runtime => "runtime.log",
            LogFile::Provider => "provider.log",
            LogFile::Tools => "tools.log",
            LogFile::Prompt => "prompt.log",
            LogFile::Panic => "panic.log",
            LogFile::Input => "input.log",
            LogFile::Output => "output.log",
            LogFile::Audit => "audit.log",
            LogFile::Agent => "agent.log",
        }
    }
}

pub fn log_path(base_dir: &Path, log_file: LogFile) -> PathBuf {
    base_dir.join(log_file.file_name())
}

pub fn prepare_log_file(base_dir: &Path, log_file: LogFile) -> io::Result<PathBuf> {
    let path = log_path(base_dir, log_file);
    prepare_log_path(&path)?;
    Ok(path)
}

pub fn open_append(base_dir: &Path, log_file: LogFile) -> io::Result<File> {
    let path = prepare_log_file(base_dir, log_file)?;
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn append_line(base_dir: &Path, log_file: LogFile, line: &str) -> io::Result<()> {
    let mut file = open_append(base_dir, log_file)?;
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
    base_dir: &Path,
    log_file: LogFile,
    session_id: &str,
    level: &str,
    module: &str,
    message: &str,
) -> io::Result<()> {
    append_text_line_with_turn(base_dir, log_file, session_id, None, level, module, message)
}

pub fn append_text_line_with_turn(
    base_dir: &Path,
    log_file: LogFile,
    session_id: &str,
    turn: Option<usize>,
    level: &str,
    module: &str,
    message: &str,
) -> io::Result<()> {
    append_line(
        base_dir,
        log_file,
        &format_text_line_with_turn(session_id, turn, level, module, message),
    )
}

pub fn append_json_line(
    base_dir: &Path,
    log_file: LogFile,
    session_id: &str,
    level: &str,
    module: &str,
    message: &str,
    extra: serde_json::Value,
) -> io::Result<()> {
    append_json_line_with_turn(
        base_dir,
        log_file,
        JsonLine {
            session_id,
            turn: None,
            level,
            module,
            message,
            extra,
        },
    )
}

pub struct JsonLine<'a> {
    pub session_id: &'a str,
    pub turn: Option<usize>,
    pub level: &'a str,
    pub module: &'a str,
    pub message: &'a str,
    pub extra: serde_json::Value,
}

pub fn append_json_line_with_turn(
    base_dir: &Path,
    log_file: LogFile,
    line: JsonLine<'_>,
) -> io::Result<()> {
    let value = json!({
        "timestamp": timestamp_rfc3339(),
        "session_id": line.session_id,
        "turn": line.turn,
        "level": line.level,
        "module": line.module,
        "message": line.message,
        "extra": line.extra,
    });
    append_line(base_dir, log_file, &value.to_string())
}
