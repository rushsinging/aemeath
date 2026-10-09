//! 任务日志文件：per-process 输出真相源（#1890 输出直绑）。
//!
//! 子进程类工具的 stdout/stderr 直接重定向到本文件（`Stdio::from`，
//! O_APPEND），零跳字节、无捕获上限、子进程退出由 OS 关 fd；非流式
//! 工具由 runtime 终态 append（唯一写入方）。`logs` 查询按文件区间读
//! （游标 = 字节偏移），token budget 截断在读取层叠加。
//!
//! 生命周期随 session：快路径完成后删除单文件；session 清理整目录。

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use share::ids::BackgroundProcessId;

/// `read_range` 返回段：字节内容 + 下一游标 + 截断标记。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskLogSegment {
    pub(crate) bytes: Vec<u8>,
    /// 游标推进到 min(start + bytes.len(), 文件大小)。
    pub(crate) next_cursor: u64,
    /// `max_bytes` 小于剩余可读量时置位（调用方可提示截断）。
    pub(crate) truncated: bool,
}

/// 会话任务日志目录：`{base}/{session_id}.background-process/`。
///
/// 平铺前缀式命名（不打散 sessions 既有平铺布局）；GC / resume 对账
/// 按 session 前缀整目录处理。
pub(crate) fn session_logs_dir(base: &Path, session_id: &str) -> PathBuf {
    base.join(format!("{session_id}.background-process"))
}

/// 单任务日志文件路径（未建文件也可推导，供恢复对账）。
pub(crate) fn task_log_path(
    base: &Path,
    session_id: &str,
    process_id: &BackgroundProcessId,
) -> PathBuf {
    session_logs_dir(base, session_id).join(format!("{}.log", process_id.as_str()))
}

/// 已创建的任务日志文件句柄（路径视图 + 终态兜底写 + 区间读）。
#[derive(Debug, Clone)]
pub(crate) struct TaskLogFile {
    path: PathBuf,
}

impl TaskLogFile {
    /// 派发即创建：建目录 + 建文件（append），返回 stdout/stderr 两个
    /// 独立 O_APPEND 写端（交错原子追加，无需锁）。
    pub(crate) fn open(
        base: &Path,
        session_id: &str,
        process_id: &BackgroundProcessId,
    ) -> std::io::Result<(Self, std::fs::File, std::fs::File)> {
        let dir = session_logs_dir(base, session_id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.log", process_id.as_str()));
        let stdout = open_append(&path)?;
        let stderr = open_append(&path)?;
        Ok((Self { path }, stdout, stderr))
    }

    /// 既有路径构造（恢复 / 对账场景，文件可能已不存在）。
    pub(crate) fn from_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// 路径推导（不建文件）。
    pub(crate) fn path_for(
        base: &Path,
        session_id: &str,
        process_id: &BackgroundProcessId,
    ) -> PathBuf {
        task_log_path(base, session_id, process_id)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// 文件实际大小（`total_written_bytes` 真相）。
    pub(crate) fn size_bytes(&self) -> u64 {
        std::fs::metadata(&self.path)
            .map(|meta| meta.len())
            .unwrap_or(0)
    }

    /// 区间读：从 `start_byte` 起最多 `max_bytes` 字节，返回下一游标。
    /// 文件不存在 / 游标超尾按空段处理（`next_cursor` 停在文件大小）。
    pub(crate) fn read_range(
        &self,
        start_byte: u64,
        max_bytes: usize,
    ) -> std::io::Result<TaskLogSegment> {
        let size = self.size_bytes();
        if start_byte >= size {
            return Ok(TaskLogSegment {
                bytes: Vec::new(),
                next_cursor: size,
                truncated: false,
            });
        }
        let file_len = size - start_byte;
        let read_len = file_len.min(max_bytes as u64) as usize;
        let file = std::fs::File::open(&self.path)?;
        use std::io::Seek as _;
        let mut reader = std::io::BufReader::new(file);
        reader.seek(std::io::SeekFrom::Start(start_byte))?;
        let mut bytes = vec![0u8; read_len];
        let mut filled = 0usize;
        while filled < read_len {
            let n = reader.read(&mut bytes[filled..])?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        bytes.truncate(filled);
        let truncated = (filled as u64) < file_len;
        Ok(TaskLogSegment {
            next_cursor: start_byte + filled as u64,
            truncated,
            bytes,
        })
    }

    /// 非流式工具 / Agent 终态兜底：append 结果文本（唯一写入方是 runtime）。
    pub(crate) fn append_terminal(&self, text: &str) -> std::io::Result<()> {
        let mut file = open_append(&self.path)?;
        file.write_all(text.as_bytes())
    }

    /// 快路径清理：输出已直接进 tool_result，完成后删除文件不留垃圾。
    pub(crate) fn remove(&self) -> std::io::Result<()> {
        std::fs::remove_file(&self.path)
    }
}

/// session 级清理：整目录删除（目录不存在视为成功）。
pub(crate) fn remove_session_logs(base: &Path, session_id: &str) -> std::io::Result<()> {
    let dir = session_logs_dir(base, session_id);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn open_append(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

#[cfg(test)]
#[path = "log_file_tests.rs"]
mod tests;
