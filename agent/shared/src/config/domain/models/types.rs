//! 多来源模型配置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Multi-source model configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsConfig {
    /// Merge mode: "merge" to combine with env/CLI settings.
    #[serde(default)]
    pub mode: String,

    /// Default source and model in "<source>/<model>" format (e.g. "zhipu/glm-5.1").
    /// Used when no --model / AEMEATH_MODEL selection is set.
    #[serde(default)]
    pub default: String,

    /// Source configurations keyed by source key (stored in JSON as `models.providers`).
    #[serde(default)]
    pub providers: HashMap<String, ProviderModelsConfig>,

    /// Guidance file overrides, keyed by glob pattern (e.g. "zhipu/*" → "~/.aemeath/guidance/glm.md").
    #[serde(default)]
    pub guidance: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    pub source_key: String,
    pub source_config: ProviderModelsConfig,
    pub model: ModelEntryConfig,
    pub driver: String,
}

/// Configuration for a single model source within models config.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ProviderModelsConfig {
    /// Base URL for the source API.
    #[serde(default, rename = "baseUrl")]
    pub base_url: String,

    /// API key for this source.
    #[serde(default, rename = "apiKey")]
    pub api_key: String,

    /// Provider driver: "openai", "anthropic", "zhipu", "litellm", "volcengine", "minimax", "mimo", "deepseek", "agnes", or "ollama".
    #[serde(default)]
    pub driver: String,

    /// Available models for this source.
    #[serde(default)]
    pub models: Vec<ModelEntryConfig>,

    /// Provider 专属 User-Agent 覆盖。
    ///
    /// - JSON 字段名固定为 `userAgent`，缺失字段视为未配置，向后兼容旧配置文件。
    /// - 该字段只属于 Provider source，不复用 legacy `api.user_agent`。
    /// - Config domain 在 UA resolver 与 Connect candidate 归一化时把空白字符串视为
    ///   未配置；持有者须经 [`ProviderModelsConfig::normalized_user_agent`] /
    ///   [`ProviderModelsConfig::with_normalized_user_agent`] 进入回退链，
    ///   **NEVER** 直接下发原始字符串。
    #[serde(default, rename = "userAgent", skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

impl ProviderModelsConfig {
    /// 获取规范化后的 Provider 专属 User-Agent。
    ///
    /// - 缺失字段或全空白字符串返回 `None`；
    /// - 非空白字段去除首尾 ASCII 空白后返回原始字符串切片；
    /// - 不在此函数内做 `HeaderValue` 合法性校验——调用方必须在 UA resolver 内串入
    ///   HeaderValue 校验并拒绝含控制字符的片段（见 Config `user_agent.rs`）。
    pub fn normalized_user_agent(&self) -> Option<&str> {
        let raw = self.user_agent.as_deref()?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            // 保留原 trimmed 视图：调用方在 HeaderValue 校验失败时仍能读取同一来源。
            Some(trimmed)
        }
    }

    /// 构造一个 user_agent 已归一化的拷贝：空白归一为 `None`，其它字段保持不变。
    ///
    /// 用于 Connect candidate 在落盘前清掉空白覆盖，避免下游误把空白当作有效配置。
    pub fn with_normalized_user_agent(&self) -> Self {
        let mut clone = self.clone();
        clone.user_agent = match self.user_agent.as_deref() {
            Some(raw) if raw.trim().is_empty() => None,
            _ => clone.user_agent,
        };
        clone
    }
}

/// A single model entry within a source
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct ModelEntryConfig {
    /// Model ID (used in API calls)
    pub id: String,

    /// Display name
    #[serde(default)]
    pub name: String,

    /// Supported input types (e.g. ["text", "image"])
    #[serde(default)]
    pub input: Vec<String>,

    /// Context window size in tokens
    #[serde(default, rename = "contextWindow")]
    pub context_window: usize,

    /// Maximum output tokens
    #[serde(default, rename = "max_tokens", alias = "maxTokens")]
    pub max_tokens: u32,

    /// Reasoning / thinking mode for this model.
    /// - `None` (default) — use CLI flag / global default
    /// - `Some(true)` — force enable thinking
    /// - `Some(false)` — force disable thinking
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,

    /// 固定推理档位。接受以下字符串之一（大小写不敏感）：
    ///
    /// | 值 | 含义 |
    /// |---|---|
    /// | `"off"` | 关闭推理/思考（canonical 表示） |
    /// | `"none"` | 关闭推理/思考——仅作 **Off 输入别名** 与 **OpenAI wire 别名**（OpenAI Responses API 以 `"none"` 表示关闭）；内部 canonical 统一为 `"off"`，**NEVER** 以 `"none"` 作为 canonical 表示 |
    /// | `"minimal"` | 最低档推理 effort |
    /// | `"low"` | 低档推理 effort |
    /// | `"medium"` | 中档推理 effort（旧 `reasoning: true` 的等价默认） |
    /// | `"high"` | 高档推理 effort |
    /// | `"xhigh"` | 超高档推理 effort |
    /// | `"max"` | 最高档推理 effort（等同于模型最大思考预算） |
    ///
    /// - `None`（默认）— 沿用 `reasoning` bool 映射（true→Medium）
    /// - `Some(level)` — 视为开启思考并取该档位，优先级高于 `reasoning`
    ///
    /// 最终档位仍会被全局 max_reasoning 上限与各 provider 能力上限双重 clamp。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,

    /// API 风格：`"responses"` 使用 OpenAI Responses API（/v1/responses），
    /// 其他值或缺省使用 Chat Completions API（/v1/chat/completions）。
    /// gpt-5.6-sol 等模型只支持 Responses API。
    #[serde(default, rename = "apiStyle", skip_serializing_if = "Option::is_none")]
    pub api_style: Option<String>,
}

impl ModelEntryConfig {
    /// 获取模型的显示标签（name 非空且不等于 id 时显示 "name (id: id)"，否则只显示 id）
    pub fn display_label(&self) -> String {
        if self.name.is_empty() || self.name == self.id {
            self.id.clone()
        } else {
            format!("{} (id: {})", self.name, self.id)
        }
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod types_tests;
