//! Harness RSI 维护 daemon 配置（`harness.rsi.*`）。

use serde::{Deserialize, Serialize};

pub(crate) fn default_interval_secs() -> u64 {
    1800
}

pub(crate) fn default_chunk_max_components() -> usize {
    8
}

/// RSI 维护 daemon 配置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RsiConfig {
    /// 总开关（false 时 daemon 不启动定时周期）。
    #[serde(default = "crate::config::ui::default_true")]
    pub enabled: bool,

    /// Timer 触发周期（秒）。
    #[serde(default = "default_interval_secs")]
    pub interval_secs: u64,

    /// 每块材料组件数上限（分块改进循环）。
    #[serde(default = "default_chunk_max_components")]
    pub chunk_max_components: usize,
}

impl Default for RsiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_interval_secs(),
            chunk_max_components: default_chunk_max_components(),
        }
    }
}

/// Harness 配置（`harness.*` 顶层节）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HarnessConfig {
    /// RSI 维护 daemon（`harness.rsi.*`）。
    #[serde(default)]
    pub rsi: RsiConfig,
}
