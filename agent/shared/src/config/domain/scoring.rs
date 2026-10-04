//! System One 评分服务配置

use serde::{Deserialize, Serialize};

pub(super) fn default_scoring_url() -> String {
    "http://127.0.0.1:8009".to_owned()
}

pub(super) fn default_scoring_model() -> String {
    "kev-latest".to_owned()
}

pub(super) fn default_scoring_timeout_ms() -> u64 {
    2_000
}

/// System One 评分服务配置：引擎端点与逐场景开关（默认全关，
/// 开关全关时行为与无评分服务完全一致）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringConfig {
    /// 评分服务 base URL（kev.serve 本地端点）。
    #[serde(default = "default_scoring_url", alias = "baseUrl")]
    pub url: String,

    /// 引擎模型名（审计 revision 用）。
    #[serde(default = "default_scoring_model")]
    pub model: String,

    /// 请求总超时（毫秒），超时即 Unavailable 回退。
    #[serde(default = "default_scoring_timeout_ms", alias = "timeoutMs")]
    pub timeout_ms: u64,

    /// 场景开关：记忆检索重排（storage memory 搜索）。
    #[serde(default, alias = "memoryRerank")]
    pub memory_rerank: bool,

    /// 场景开关：Skill 匹配（skills / SkillTool）。
    #[serde(default, alias = "skillMatch")]
    pub skill_match: bool,

    /// 场景开关：权限/风险预筛（policy）。
    #[serde(default, alias = "policyTriage")]
    pub policy_triage: bool,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            url: default_scoring_url(),
            model: default_scoring_model(),
            timeout_ms: default_scoring_timeout_ms(),
            memory_rerank: false,
            skill_match: false,
            policy_triage: false,
        }
    }
}

#[cfg(test)]
#[path = "scoring_tests.rs"]
mod tests;
