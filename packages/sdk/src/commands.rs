//! Slash Command Published Language exposed to delivery clients.
//!
//! The owner is the Tools BC. SDK re-exports the exact types and ports instead
//! of defining a second descriptor, parser, or route model.

pub use tools::published::command::{
    ApplicationControlCommand, ApplicationControlTarget, CommandArgumentSchema, CommandCatalogPort,
    CommandCompletion, CommandDescriptor, CommandMechanism, CommandName, CommandParseError,
    CommandRoute, CommandRouterPort, CommandTarget, ParsedArguments, SkillRequestCommand,
    SlashInput,
};
pub use tools::published::snapshot_query::{SnapshotQueryCommand, SnapshotQueryTarget};

// ─── Command execution result views ───

/// Result of estimating context window usage.
#[derive(Debug, Clone, Copy)]
pub struct ContextEstimate {
    /// Estimated token count for all messages
    pub estimated_tokens: usize,
    /// Estimated tokens for system prompt only
    pub system_tokens: usize,
    /// Available context window size
    pub context_size: usize,
    /// Usage percentage (0-100)
    pub usage_percentage: f64,
}

// ─── Model switch result ───

/// 单档位映射条目（#2032）：用户请求档位 → 当前模型阶梯 resolve 后的
/// 实际生效档位（权威计算在 runtime `ModelInfo::resolve_reasoning`，
/// TUI 只渲染，NEVER 视图层复制 resolve 逻辑）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReasoningEffortMapping {
    pub requested: share::reasoning::ReasoningLevel,
    pub effective: share::reasoning::ReasoningLevel,
}

/// Result of switching the active model.
#[derive(Debug, Clone)]
pub struct ModelSwitchResult {
    pub display_name: String,
    pub context_window: usize,
    pub reasoning_active: Option<bool>,
    /// 切换后生效的 reasoning 深度（#1616 状态栏展示）。旧构造方缺省为 `None`。
    pub reasoning_level: Option<share::reasoning::ReasoningLevel>,
    /// 切换后模型的全档映射表（#2032 /think 对话框逐档映射标注）。
    /// 旧构造方缺省为 `None`（按无标注处理）。
    pub reasoning_effort_map: Option<Vec<ReasoningEffortMapping>>,
}
