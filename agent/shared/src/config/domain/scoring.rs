//! System One 评分服务配置：仅保留逐场景开关。

use serde::{Deserialize, Serialize};

/// System One 评分服务配置：四个场景开关（默认全关，开关全关时行为与无评分
/// 服务完全一致）。HTTP 端点配置（url/model/timeout_ms）已随设计 §4.3 退役，
/// 旧配置残留键被忽略而非报错。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScoringConfig {
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
