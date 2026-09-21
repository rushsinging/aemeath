use serde::{Deserialize, Deserializer, Serialize};

const fn default_enabled() -> bool {
    true
}

fn default_auto_compact_failure_limit() -> u8 {
    3
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
            compact_model: None,
        }
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;
