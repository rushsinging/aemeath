//! 压缩后文件恢复与消息重组
//!
//! 压缩完成后，恢复最近读取的文件内容以保持上下文连贯；
//! 清理孤立工具调用对；重组最终消息列表。
//!
//! ## 模块结构
//! - `restore_files` — 文件恢复附件构建
//! - `sanitize_pairs` — ToolUse/ToolResult 配对修复
//! - `assemble` — 最终消息组装与角色交替修复

mod restore_files;
mod sanitize_pairs;
mod assemble;

pub use restore_files::build_file_restoration;
pub use restore_files::{POST_COMPACT_MAX_FILES, POST_COMPACT_MAX_TOKENS_PER_FILE, POST_COMPACT_TOKEN_BUDGET};
pub use sanitize_pairs::sanitize_tool_pairs;
pub use assemble::{assemble_compacted, assemble_compacted_with_files, fix_role_alternation};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
