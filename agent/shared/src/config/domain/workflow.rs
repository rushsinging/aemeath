//! Workflow Graph 配置（#2085）：MVP 总开关，完全交付前默认关闭。

use serde::{Deserialize, Serialize};

/// Workflow 配置段。
///
/// `enabled = false`（默认）时 composition 不注入 Workflow backend，六个
/// `workflow_*` Tool 不进入 catalog；开启后注入内存 backend。Workflow
/// MVP 验收（#2091）通过前保持默认关闭。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// 六个 `workflow_*` Tool 的总开关；默认 `false`。
    #[serde(default)]
    pub enabled: bool,
    /// 自动启动段（KEV 评分判定）；默认关闭。
    #[serde(default)]
    pub autostart: WorkflowAutostartConfig,
}

/// KEV（System One）评分自动启动 Workflow 的配置段。
///
/// 语义：turn 边界对最新 user message 单题评分，达阈值且无 Active Workflow
/// 时由 Runtime 直接建出 `plan → complete` 初始骨架并注入一条 SystemMessage
/// （一次性指令，非 Reminder）；后续节点由 LLM 经 `workflow_replan` 补齐。
/// daemon 不可用 / 超时一律 fail-open（静默不启动）。评分只增不锁：LLM 手动
/// `workflow_create` 永远允许。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct WorkflowAutostartConfig {
    /// 自动启动总开关；默认 `false`（且须 `workflow.enabled = true` 才生效）。
    #[serde(default)]
    pub enabled: bool,
    /// 评分阈值，闭区间放行（`score >= threshold`）；默认 `0.6`，待校准。
    #[serde(default = "default_autostart_threshold")]
    pub threshold: f64,
    /// 评分超时（毫秒），超时按无评分处理（fail-open）；默认 `2000`。
    #[serde(default = "default_autostart_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_autostart_threshold() -> f64 {
    0.6
}

fn default_autostart_timeout_ms() -> u64 {
    2000
}
