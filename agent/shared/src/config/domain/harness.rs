//! Harness RSI 维护 daemon 配置（`harness.rsi.*`）。

use serde::{Deserialize, Serialize};

pub(crate) fn default_interval_secs() -> u64 {
    28_800
}

pub(crate) fn default_chunk_max_components() -> usize {
    8
}

pub(crate) fn default_evidence_initial_window_days() -> u64 {
    30
}

pub(crate) fn default_evidence_max_runs_per_cycle() -> usize {
    50
}

pub(crate) fn default_evidence_block_runs() -> usize {
    5
}

/// RSI 改进执行模式（#2078 拍板修订）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RsiExecutionMode {
    /// 单阶段：证据 → 一次提案 → 落盘（v1 行为，默认保持兼容）。
    #[default]
    SinglePhase,
    /// 诊断优先：每块 1 次诊断只产出 issue，跨块汇总，执行派发由后续
    /// 强度门禁（#2092）决定——本模式当前**不落盘**。
    DiagnoseOnly,
    /// 两阶段：宿主路由候选 → 诊断选择固定组件 → 执行只写该组件正文。
    TwoPhase,
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

    /// 每周期步长上限：最多处理的 run 数（超出留待下周期）。
    /// 旧配置键 `evidence_max_sessions_per_cycle` 只是兼容 alias。
    #[serde(
        default = "default_evidence_max_runs_per_cycle",
        alias = "evidence_max_sessions_per_cycle"
    )]
    pub evidence_max_runs_per_cycle: usize,

    /// 数据源分块粒度：每块 run 数上限。
    /// 旧配置键 `evidence_block_sessions` 只是兼容 alias。
    #[serde(
        default = "default_evidence_block_runs",
        alias = "evidence_block_sessions"
    )]
    pub evidence_block_runs: usize,

    /// 改进执行模式（#2078）：single_phase（默认，一次提案即落盘）/
    /// diagnose_only（每块仅诊断 + 跨块汇总，不落盘，执行由 #2092 派发）。
    #[serde(default)]
    pub execution_mode: RsiExecutionMode,
}

impl Default for RsiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model: String::new(),
            interval_secs: default_interval_secs(),
            chunk_max_components: default_chunk_max_components(),
            evidence_initial_window_days: default_evidence_initial_window_days(),
            evidence_max_runs_per_cycle: default_evidence_max_runs_per_cycle(),
            evidence_block_runs: default_evidence_block_runs(),
            execution_mode: RsiExecutionMode::default(),
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
