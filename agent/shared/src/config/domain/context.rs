use serde::{Deserialize, Deserializer, Serialize};

const fn default_enabled() -> bool {
    true
}

fn default_auto_compact_failure_limit() -> u8 {
    3
}

/// `auto_compact_threshold_ratio` 的安全区间下限。
///
/// 低于该值时阈值过小，任意一轮对话即可能恒触发 auto-compact，
/// 形成 compact 风暴直至熔断（#1626 教训）。
pub const AUTO_COMPACT_THRESHOLD_RATIO_MIN: f64 = 0.5;

/// `auto_compact_threshold_ratio` 的安全区间上限。
///
/// 高于该值时估算误差缓冲过薄——compact 请求自身占用的上下文可能
/// 超出窗口，触发后已无空间完成压缩。
pub const AUTO_COMPACT_THRESHOLD_RATIO_MAX: f64 = 0.95;

fn default_auto_compact_threshold_ratio() -> f64 {
    0.8
}

/// 归一化 auto-compact 触发阈值比例：clamp 到
/// [`AUTO_COMPACT_THRESHOLD_RATIO_MIN`, `AUTO_COMPACT_THRESHOLD_RATIO_MAX`]。
pub(crate) fn clamp_auto_compact_threshold_ratio(ratio: f64) -> f64 {
    ratio.clamp(
        AUTO_COMPACT_THRESHOLD_RATIO_MIN,
        AUTO_COMPACT_THRESHOLD_RATIO_MAX,
    )
}

/// 归一化 compact 模型 selection：去除首尾空白，空串归一化为"未配置"。
pub(crate) fn normalize_compact_model_selection(selection: String) -> Option<String> {
    let trimmed = selection.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn deserialize_compact_model<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let selection = Option::<String>::deserialize(deserializer)?;
    Ok(selection.and_then(normalize_compact_model_selection))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    #[serde(default = "default_enabled")]
    pub snip_enabled: bool,
    #[serde(default = "default_enabled")]
    pub microcompact_enabled: bool,
    #[serde(default = "default_auto_compact_failure_limit")]
    pub auto_compact_failure_limit: u8,
    /// Auto-compact 触发阈值占 effective context window 的比例。
    ///
    /// 存储原始配置值，读取时经 [`clamp_auto_compact_threshold_ratio`]
    /// 归一化到 `[0.5, 0.95]`（防 compact 风暴与缓冲归零）。
    #[serde(
        default = "default_auto_compact_threshold_ratio",
        alias = "autoCompactThresholdRatio"
    )]
    pub auto_compact_threshold_ratio: f64,
    /// Compact 专用模型 selection（`<source>/<model>`）。
    ///
    /// `None` 表示跟随当前会话模型；解析失败不是"未配置"，由 Runtime 的
    /// compact 模型解析器区分并显式报错。
    #[serde(
        default,
        alias = "compactModel",
        deserialize_with = "deserialize_compact_model"
    )]
    pub compact_model: Option<String>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            snip_enabled: true,
            microcompact_enabled: true,
            auto_compact_failure_limit: default_auto_compact_failure_limit(),
            auto_compact_threshold_ratio: default_auto_compact_threshold_ratio(),
            compact_model: None,
        }
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;
