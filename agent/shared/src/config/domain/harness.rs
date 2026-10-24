//! Harness RSI 维护 daemon 配置（`harness.rsi.*`）。

use serde::{Deserialize, Serialize};

pub(crate) fn default_interval_secs() -> u64 {
    1800
}

pub(crate) fn default_chunk_max_components() -> usize {
    8
}

pub(crate) fn default_evidence_initial_window_days() -> u64 {
    30
}

pub(crate) fn default_evidence_max_sessions_per_cycle() -> usize {
    20
}

pub(crate) fn default_evidence_block_sessions() -> usize {
    5
}

/// RSI 维护 daemon 配置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RsiConfig {
    /// 总开关（false 时 daemon 不启动定时周期）。
    #[serde(default = "crate::config::ui::default_true")]
    pub enabled: bool,

    /// RSI 独立模型档（"<source>/<model>" 格式；空=继承 models.default，
    /// 对齐 memory.reflection.model 范式——改进模型可与对话模型分离）。
    #[serde(default)]
    pub model: String,

    /// Timer 触发周期（秒）。
    #[serde(default = "default_interval_secs")]
    pub interval_secs: u64,

    /// 每块材料组件数上限（**待废弃**：v1 按组件分块遗留；主分块单位是
    /// 数据源块，本参数降级为单块内组件面体积兜底，04 分片 §2A.5）。
    #[serde(default = "default_chunk_max_components")]
    pub chunk_max_components: usize,

    /// 首次（无 checkpoint）证据窗口（天）。
    #[serde(default = "default_evidence_initial_window_days")]
    pub evidence_initial_window_days: u64,

    /// 每周期步长上限：最多处理的 session 数（超出留待下周期）。
    #[serde(default = "default_evidence_max_sessions_per_cycle")]
    pub evidence_max_sessions_per_cycle: usize,

    /// 数据源分块粒度：每块 session 数上限。
    #[serde(default = "default_evidence_block_sessions")]
    pub evidence_block_sessions: usize,
}

impl Default for RsiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: String::new(),
            interval_secs: default_interval_secs(),
            chunk_max_components: default_chunk_max_components(),
            evidence_initial_window_days: default_evidence_initial_window_days(),
            evidence_max_sessions_per_cycle: default_evidence_max_sessions_per_cycle(),
            evidence_block_sessions: default_evidence_block_sessions(),
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
