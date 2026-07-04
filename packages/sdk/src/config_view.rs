//! ConfigView — TUI 层只读的配置视图。
//!
//! 从 runtime 的 ConfigSnapshot 提取 TUI 需要的字段。
//! TUI NEVER 直接读 config/env，只通过此类型获取展示信息。

use serde::{Deserialize, Serialize};

/// TUI 层需要的配置视图。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigView {
    /// 当前模型名称（如 "Zhipu/glm-5.2"）
    pub model_name: String,
    /// 当前 provider
    pub provider: Option<String>,
    /// API key 是否已配置（不暴露实际值）
    pub has_api_key: bool,
    /// API key 前 8 位（用于 /doctor 显示）
    pub api_key_preview: Option<String>,
    /// Permission mode
    pub permission_mode: String,
    /// Markdown 渲染开关
    pub markdown: bool,
    /// verbose 模式
    pub verbose: bool,
    /// Context size（0 = auto-resolve）
    pub context_size: usize,
    /// Logging level
    pub logging_level: String,
}
