//! 会话元数据状态

use ::runtime::api::core::config::MemoryConfig;
use std::path::PathBuf;

/// 会话相关信息（不含基础设施引用）
#[derive(Debug)]
pub(crate) struct SessionState {
    pub session_id: String,
    pub cwd: PathBuf,
    pub session_created_at: Option<String>,
    pub cached_sessions: Vec<(String, String)>,
    pub current_model_display: String,
    pub memory_config: MemoryConfig,
}
