//! System One 评分服务配置：逐场景开关 + 评分事件保留天数。

pub use super::constants::{
    DEFAULT_EVENT_RETENTION_DAYS, DEFAULT_SCORING_CONNECT_TIMEOUT_SECS,
    DEFAULT_SCORING_INFER_TIMEOUT_SECS,
};
use serde::{Deserialize, Serialize};

/// `ScoringConfig::enabled` 的 serde 默认值（保持既有配置文件兼容：缺省即开）。
fn default_enabled() -> bool {
    true
}

/// `ScoringConfig::event_retention_days` 的 serde 默认值（与
/// [`DEFAULT_EVENT_RETENTION_DAYS`] 同源，见该常量文档）。
fn default_event_retention_days() -> u32 {
    DEFAULT_EVENT_RETENTION_DAYS
}

fn default_connect_timeout_secs() -> u64 {
    DEFAULT_SCORING_CONNECT_TIMEOUT_SECS
}

fn default_infer_timeout_secs() -> u64 {
    DEFAULT_SCORING_INFER_TIMEOUT_SECS
}

impl Default for ScoringConfig {
    /// 总闸门默认开（场景开关默认全关）；`enabled=false` 仅经显式配置/env。
    fn default() -> Self {
        Self {
            enabled: true,
            memory_rerank: false,
            skill_match: false,
            policy_triage: false,
            memory_recall: false,
            event_retention_days: DEFAULT_EVENT_RETENTION_DAYS,
            socket_path: None,
            connect_timeout_secs: DEFAULT_SCORING_CONNECT_TIMEOUT_SECS,
            infer_timeout_secs: DEFAULT_SCORING_INFER_TIMEOUT_SECS,
        }
    }
}

/// System One 评分服务配置：四个场景开关（默认全关）+ 事件保留天数 + daemon 客户端超时 / 可选 socket_path（#1935）。
/// HTTP 端点配置（url/model/timeout_ms）已随设计 §4.3 退役，
/// 旧配置残留键被忽略而非报错。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringConfig {
    /// 总闸门（默认开，opt-out）：`false` 时无视全部场景开关走零成本路径
    ///（不读模型目录、不装配、无提醒）。场景开关仍各自决定启用范围。
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// 场景开关：记忆检索重排（storage memory 搜索）。
    #[serde(default, alias = "memoryRerank")]
    pub memory_rerank: bool,

    /// 场景开关：Skill 匹配（skills / SkillTool）。
    #[serde(default, alias = "skillMatch")]
    pub skill_match: bool,

    /// 场景开关：权限/风险预筛（policy）。
    #[serde(default, alias = "policyTriage")]
    pub policy_triage: bool,

    /// 场景开关：per-message 记忆主动召回（reminder 注入，含 token 预算与
    /// 相关性阈值门）。
    #[serde(default, alias = "memoryRecall")]
    pub memory_recall: bool,

    /// 评分事件 `events/` 日切 segment 保留天数（默认 30；`0`=仅禁用过期
    /// segment 的 GC，**NEVER** 表示关闭事件写入）。
    #[serde(default = "default_event_retention_days", alias = "eventRetentionDays")]
    pub event_retention_days: u32,

    /// 可选覆盖 UDS 路径；缺省为 `{agents_dir}/scoring/scoring.sock`。
    #[serde(default, alias = "socketPath")]
    pub socket_path: Option<String>,

    /// 连接（含惰性拉起等待）超时秒数（默认 2）。
    #[serde(default = "default_connect_timeout_secs", alias = "connectTimeoutSecs")]
    pub connect_timeout_secs: u64,

    /// 单次推理超时秒数（默认 30）。
    #[serde(default = "default_infer_timeout_secs", alias = "inferTimeoutSecs")]
    pub infer_timeout_secs: u64,
}

#[cfg(test)]
#[path = "scoring_tests.rs"]
mod tests;
