//! 日志文件管理与格式化输出
//!
//! 路径无关：所有接受文件路径的函数通过 `base_dir: &Path` 参数传入。
//!
//! # 日志文件职责
//!
//! | 文件 | 职责 | 内容 |
//! |------|------|------|
//! | `aemeath.log` | **应用主日志**：所有模块的结构化运行日志 | env_logger pipe 接收全部 `log::*` 输出 |
//! | `input.log` | **LLM 输入快照** | 每次 API 调用的新增 messages 摘要、system blocks、tool schemas |
//! | `output.log` | **LLM 完整输出** | 模型返回的完整 content blocks、token 用量、耗时 |
//! | `tool.log` | **工具调用记录** | 工具调用请求参数 + 工具执行结果（完整输出） |
//! | `agent.log` | **已废弃**：Agent 对话审计日志 | 无写入点，保留枚举兼容 |
//! | `panic.log` | **Panic 崩溃日志** | panic 信息 + backtrace |

pub mod json;
pub mod rotation;
pub mod text;

pub use json::JsonLogger;
pub use rotation::{is_rotated_log_path, rotated_path, timestamp_rfc3339};
pub use text::{
    append_json_line, append_json_line_with_turn, append_line, append_text_line,
    append_text_line_with_turn, format_text_line, format_text_line_with_turn, open_append,
    prepare_log_file, LogFile,
};

pub const LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const LOG_MAX_BACKUPS: usize = 5;
pub const LOG_RETENTION_DAYS: u64 = 30;
