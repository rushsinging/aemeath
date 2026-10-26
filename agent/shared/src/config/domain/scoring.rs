//! System One 评分服务配置：逐场景开关 + 评分事件保留天数 + backend 驱动选择。

pub use super::constants::{
    DEFAULT_BAILIAN_SCORING_MODEL, DEFAULT_EVENT_RETENTION_DAYS,
    DEFAULT_SCORING_CONNECT_TIMEOUT_SECS, DEFAULT_SCORING_INFER_TIMEOUT_SECS,
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

fn default_bailian_model() -> String {
    DEFAULT_BAILIAN_SCORING_MODEL.to_string()
}

/// 评分后端驱动：local（UDS daemon，现状默认）或 bailian（百炼决策模型 HTTPS）。
///
/// bailian 端点为 TypeSafe System One 协议（`POST {endpoint}`，题型
/// noul/choice/score），与本地评分语言同构；凭证独立配置，
/// **NEVER** 与 `models.providers` 混用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScoringBackend {
    /// 本地 systemone daemon（UDS）。
    #[default]
    Local,
    /// 百炼决策模型（`decision-model-preview`）。
    Bailian,
}

/// bailian 驱动连接配置（#2095）：与 `models.providers` 独立，不混用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BailianScoringConfig {
    /// API Key（Bearer 鉴权，backend=bailian 时必填）。
    #[serde(default, alias = "apiKey")]
    pub api_key: Option<String>,

    /// 基础 URL：请求端点 = `base_url + "/systemone"`（拼接前去尾斜杠）。
    #[serde(default, alias = "baseUrl")]
    pub base_url: Option<String>,

    /// 完整端点 URL：设置时优先于 `base_url` 拼接。
    #[serde(default, alias = "fullUrl")]
    pub full_url: Option<String>,

    /// 模型名（默认 [`DEFAULT_BAILIAN_SCORING_MODEL`]）。
    #[serde(default = "default_bailian_model")]
    pub model: String,
}

impl Default for BailianScoringConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            full_url: None,
            model: DEFAULT_BAILIAN_SCORING_MODEL.to_string(),
        }
    }
}

/// bailian 端点解析失败原因（装配层 fail-open 提醒用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BailianConfigError {
    /// `base_url` 与 `full_url` 均未配置。
    MissingEndpoint,
    /// `api_key` 未配置。
    MissingApiKey,
}

impl std::fmt::Display for BailianConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingEndpoint => formatter.write_str("base_url 与 full_url 均未配置"),
            Self::MissingApiKey => formatter.write_str("api_key 未配置"),
        }
    }
}

impl std::error::Error for BailianConfigError {}

impl BailianScoringConfig {
    /// 解析请求端点：`full_url` 优先；否则 `base_url` 去尾斜杠后拼 `/systemone`。
    pub fn resolve_endpoint(&self) -> Result<String, BailianConfigError> {
        if let Some(full_url) = self.full_url.as_deref().map(str::trim) {
            if !full_url.is_empty() {
                return Ok(full_url.to_string());
            }
        }
        if let Some(base_url) = self.base_url.as_deref().map(str::trim) {
            if !base_url.is_empty() {
                return Ok(format!("{}/systemone", base_url.trim_end_matches('/')));
            }
        }
        Err(BailianConfigError::MissingEndpoint)
    }

    /// 装配前完整性校验：api_key 与端点（base_url/full_url 至少其一）必须齐备。
    pub fn validate(&self) -> Result<(), BailianConfigError> {
        self.resolve_endpoint()?;
        if self
            .api_key
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
        {
            return Err(BailianConfigError::MissingApiKey);
        }
        Ok(())
    }
}

impl Default for ScoringConfig {
    /// 总闸门默认开（场景开关默认全关）；`enabled=false` 仅经显式配置/env。
    /// backend 默认 local（既有行为零破坏）。
    fn default() -> Self {
        Self {
            enabled: true,
            backend: ScoringBackend::Local,
            bailian: BailianScoringConfig::default(),
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

/// System One 评分服务配置：四个场景开关（默认全关）+ 事件保留天数 + daemon 客户端超时 / 可选 socket_path（#1935）+ backend 驱动选择（#2095）。
/// 旧 jev_http 线格式的 HTTP 端点配置（顶层 url/model/timeout_ms）已随设计 §4.3 退役，
/// 残留键被忽略而非报错；现行 bailian 驱动配置在独立的 `bailian` 子对象内。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringConfig {
    /// 总闸门（默认开，opt-out）：`false` 时无视全部场景开关走零成本路径
    ///（不读模型目录、不装配、无提醒）。场景开关仍各自决定启用范围。
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// 评分后端驱动（默认 local；bailian 见 [`ScoringBackend`]）。
    #[serde(default)]
    pub backend: ScoringBackend,

    /// bailian 驱动连接配置（backend=bailian 时消费；凭证独立，不与
    /// `models.providers` 混用）。
    #[serde(default)]
    pub bailian: BailianScoringConfig,

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
