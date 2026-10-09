//! System One 评分服务配置：仅保留逐场景开关。

use serde::{Deserialize, Serialize};

/// `ScoringConfig::enabled` 的 serde 默认值（保持既有配置文件兼容：缺省即开）。
fn default_enabled() -> bool {
    true
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
        }
    }
}

/// System One 评分服务配置：四个场景开关（默认全关，开关全关时行为与无评分
/// 服务完全一致）。HTTP 端点配置（url/model/timeout_ms）已随设计 §4.3 退役，
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
}

#[cfg(test)]
#[path = "scoring_tests.rs"]
mod tests;
