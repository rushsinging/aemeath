//! Runtime 引擎配置

use serde::{Deserialize, Serialize};

pub(super) fn default_tool_background_threshold_secs() -> u64 {
    10
}

/// Runtime 引擎分段配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// tool call 前台等待阈值（秒）：超过即自动转后台任务（占位结果 + 异步回注）。
    /// `0` 表示禁用后台化（纯快路径，行为与现状一致）。随 `RunConfigSnapshot` 冻结。
    #[serde(
        default = "default_tool_background_threshold_secs",
        alias = "toolBackgroundThresholdSecs"
    )]
    pub tool_background_threshold_secs: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            tool_background_threshold_secs: default_tool_background_threshold_secs(),
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
