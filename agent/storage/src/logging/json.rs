use serde_json::json;
use share::config::logging::LoggingConfig;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use super::rotation::rotate_if_needed;
use super::rotation::timestamp_rfc3339;

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
