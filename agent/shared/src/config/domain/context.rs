use serde::{Deserialize, Deserializer, Serialize};

const fn default_enabled() -> bool {
    true
}

fn default_auto_compact_failure_limit() -> u8 {
    3
}

pub use super::constants::{AUTO_COMPACT_THRESHOLD_RATIO_MAX, AUTO_COMPACT_THRESHOLD_RATIO_MIN};

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
    /// Compact 调用的推理档位（off/minimal/low/medium/high/xhigh/max）。
    ///
    /// 未配置默认 `Low`：实测（60 题语义决策类题库）三档续接正确率均 90%+
    /// 且无显著差异，成本差 2.7×；需要更深细节时用 `--reasoning max`。
    #[serde(default, alias = "compactReasoning")]
    pub compact_reasoning: Option<String>,
    /// Compact 调用的输出预算（token）。
    ///
    /// 默认 65536：max 档思考会吃掉大量 budget，16K 预算会让正文为空
    /// （生产实测 22 块降 10 块）。实际生效值仍受模型 `max_tokens` 上限约束
    /// ——配置该值后请同步确认模型配置的 max_tokens 足够大。
    #[serde(default, alias = "compactMaxOutputTokens")]
    pub compact_max_output_tokens: Option<u32>,
    /// Compact map 阶段并发上限（1..=10，默认 5）。
    ///
    /// 块数多时并发越高墙钟越短，但要受 provider 速率限制约束。
    #[serde(default, alias = "compactConcurrency")]
    pub compact_concurrency: Option<usize>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            snip_enabled: true,
            microcompact_enabled: true,
            auto_compact_failure_limit: default_auto_compact_failure_limit(),
            auto_compact_threshold_ratio: default_auto_compact_threshold_ratio(),
            compact_model: None,
            compact_reasoning: None,
            compact_max_output_tokens: None,
            compact_concurrency: None,
        }
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;
