//! Chat Provider driver 抽象：不同供应商的推理字段差异化处理

use crate::ports::ReasoningLevel;
use crate::{ProviderDriverKind, ReasoningCapability, ReasoningMappingKind};

use super::ReasoningConfig;

fn effort_capability(maximum: ReasoningLevel) -> ReasoningCapability {
    ReasoningCapability::new(
        [
            ReasoningLevel::Off,
            ReasoningLevel::Low,
            ReasoningLevel::Medium,
            ReasoningLevel::High,
            ReasoningLevel::Xhigh,
            ReasoningLevel::Max,
        ]
        .into_iter()
        .filter(|level| *level <= maximum),
        ReasoningMappingKind::Effort,
    )
    .expect("driver capability includes off")
}

fn toggle_capability(on_level: ReasoningLevel) -> ReasoningCapability {
    ReasoningCapability::new(
        [ReasoningLevel::Off, on_level],
        ReasoningMappingKind::ThinkingToggle,
    )
    .expect("toggle capability includes off")
}

pub trait ChatApiDriver: Send + Sync {
    fn max_tokens_field(&self) -> &'static str {
        "max_tokens"
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    );

    /// 将 ReasoningLevel 转为 wire 上的 effort 字符串。
    /// 默认同 as_str()；OpenAI 等供应商可将 Off 映射为 "none"。
    fn wire_effort(&self, level: ReasoningLevel) -> &'static str {
        level.as_str()
    }

    /// 将 legacy effort 字符串投影到唯一 capability 声明允许的档位。
    fn clamp_effort<'a>(&self, effort: &'a str) -> &'a str {
        ReasoningLevel::parse(effort)
            .map(|requested| self.wire_effort(self.reasoning_capability().resolve(requested)))
            .unwrap_or(effort)
    }

    fn reasoning_capability(&self) -> crate::ReasoningCapability;

    fn max_reasoning_level(&self) -> ReasoningLevel {
        self.reasoning_capability().maximum()
    }
}

#[derive(Debug)]
pub struct OpenAiDriver;

#[derive(Debug)]
pub struct ZhipuDriver;

#[derive(Debug)]
pub struct LiteLlmDriver;

#[derive(Debug)]
pub struct VolcengineDriver;

#[derive(Debug)]
pub struct MinimaxDriver;

#[derive(Debug)]
pub struct MimoDriver;

#[derive(Debug)]
pub struct DeepSeekDriver;

#[derive(Debug)]
pub struct AgnesDriver;

impl ChatApiDriver for OpenAiDriver {
    fn reasoning_capability(&self) -> ReasoningCapability {
        ReasoningCapability::new(
            [
                ReasoningLevel::Off,
                ReasoningLevel::Minimal,
                ReasoningLevel::Low,
                ReasoningLevel::Medium,
                ReasoningLevel::High,
                ReasoningLevel::Xhigh,
                ReasoningLevel::Max,
            ],
            ReasoningMappingKind::Effort,
        )
        .expect("OpenAI capability includes off")
    }

    fn wire_effort(&self, level: ReasoningLevel) -> &'static str {
        match level {
            ReasoningLevel::Off => "none",
            other => other.as_str(),
        }
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        _reasoning_enabled: bool,
    ) {
        if let Some(ReasoningConfig::Object(reasoning)) = reasoning_config {
            request_body["reasoning"] = reasoning.clone();
        }
    }
}

impl ChatApiDriver for ZhipuDriver {
    fn reasoning_capability(&self) -> ReasoningCapability {
        effort_capability(ReasoningLevel::Max)
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            _ => reasoning_enabled,
        };
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({"type": thinking_type});

        // thinking 开启时附带 reasoning_effort 字段（仅 GLM-5.2 生效）。
        // 服务端兼容映射：low/medium → high, xhigh → max。
        if enabled {
            if let Some(effort) = reasoning_config.and_then(|c| c.as_effort()) {
                request_body["reasoning_effort"] = serde_json::Value::String(effort);
            }
        }
    }
}

impl ChatApiDriver for LiteLlmDriver {
    fn reasoning_capability(&self) -> ReasoningCapability {
        effort_capability(ReasoningLevel::Max)
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        _reasoning_enabled: bool,
    ) {
        // LiteLLM proxy does not support the `reasoning` parameter.
        // Extract effort and pass it as top-level `reasoning_effort` instead,
        // which LiteLLM forwards to the upstream OpenAI-compatible endpoint.
        if let Some(effort) = reasoning_config.and_then(|c| c.as_effort()) {
            request_body["reasoning_effort"] = serde_json::Value::String(effort);
        }
    }
}

impl ChatApiDriver for VolcengineDriver {
    fn reasoning_capability(&self) -> ReasoningCapability {
        effort_capability(ReasoningLevel::Medium)
    }

    fn max_tokens_field(&self) -> &'static str {
        "max_output_tokens"
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        // Volcengine 使用与 Zhipu 相同的 thinking.type 字段控制推理开关。
        // 优先使用 reasoning_config 中的显式配置，其次使用 reasoning_enabled 标志。
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            Some(ReasoningConfig::ThinkingBudget(_)) => true,
            Some(ReasoningConfig::Object(_)) => {
                // Object 类型（如 {"effort": "medium"}）直接透传。
                if let Some(ReasoningConfig::Object(reasoning)) = reasoning_config {
                    request_body["reasoning"] = reasoning.clone();
                }
                return;
            }
            None => reasoning_enabled,
        };
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({"type": thinking_type});
    }
}

impl ChatApiDriver for MinimaxDriver {
    fn reasoning_capability(&self) -> ReasoningCapability {
        toggle_capability(ReasoningLevel::Medium)
    }

    fn max_tokens_field(&self) -> &'static str {
        "max_completion_tokens"
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        let thinking_type = match reasoning_config {
            Some(ReasoningConfig::Object(value)) => value
                .get("type")
                .and_then(|v| v.as_str())
                .filter(|kind| matches!(*kind, "disabled" | "adaptive"))
                .unwrap_or(if reasoning_enabled {
                    "adaptive"
                } else {
                    "disabled"
                }),
            Some(ReasoningConfig::Bool(value)) => {
                if *value {
                    "adaptive"
                } else {
                    "disabled"
                }
            }
            Some(ReasoningConfig::ThinkingBudget(_)) => "adaptive",
            None => {
                if reasoning_enabled {
                    "adaptive"
                } else {
                    "disabled"
                }
            }
        };
        request_body["thinking"] = serde_json::json!({ "type": thinking_type });
        request_body["reasoning_split"] = serde_json::Value::Bool(true);
    }
}

impl ChatApiDriver for MimoDriver {
    fn reasoning_capability(&self) -> ReasoningCapability {
        toggle_capability(ReasoningLevel::Medium)
    }

    fn max_tokens_field(&self) -> &'static str {
        "max_completion_tokens"
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        _reasoning_enabled: bool,
    ) {
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            Some(ReasoningConfig::ThinkingBudget(_)) | Some(ReasoningConfig::Object(_)) => true,
            None => true,
        };
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({ "type": thinking_type });
    }
}

impl ChatApiDriver for DeepSeekDriver {
    fn reasoning_capability(&self) -> ReasoningCapability {
        effort_capability(ReasoningLevel::Max)
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            _ => reasoning_enabled,
        };
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({"type": thinking_type});

        // thinking 开启时附带 reasoning_effort 顶层字段。
        // DeepSeek 仅认 high/max，服务端做兼容映射：
        // low/medium → high, xhigh → max。
        if enabled {
            if let Some(effort) = reasoning_config.and_then(|c| c.as_effort()) {
                request_body["reasoning_effort"] = serde_json::Value::String(effort);
            }
        }
    }
}

impl ChatApiDriver for AgnesDriver {
    fn reasoning_capability(&self) -> ReasoningCapability {
        toggle_capability(ReasoningLevel::Medium)
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        // Agnes 使用 vLLM chat_template_kwargs.enable_thinking 控制思考开关。
        // 仅支持开/关，不支持 effort 分级。
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            Some(ReasoningConfig::ThinkingBudget(_)) | Some(ReasoningConfig::Object(_)) => true,
            None => reasoning_enabled,
        };
        request_body["chat_template_kwargs"] = serde_json::json!({ "enable_thinking": enabled });
    }
}

pub(crate) fn driver_for_provider_driver(
    driver: ProviderDriverKind,
) -> Box<dyn ChatApiDriver + Send + Sync> {
    match driver {
        ProviderDriverKind::OpenAI => Box::new(OpenAiDriver),
        ProviderDriverKind::Zhipu => Box::new(ZhipuDriver),
        ProviderDriverKind::LiteLLM => Box::new(LiteLlmDriver),
        ProviderDriverKind::Volcengine => Box::new(VolcengineDriver),
        ProviderDriverKind::Minimax => Box::new(MinimaxDriver),
        ProviderDriverKind::Mimo => Box::new(MimoDriver),
        ProviderDriverKind::DeepSeek => Box::new(DeepSeekDriver),
        ProviderDriverKind::Agnes => Box::new(AgnesDriver),
        // Ollama 有专用 OllamaProvider，不经此 OpenAI 兼容驱动；兜底走 OpenAI 驱动。
        ProviderDriverKind::Anthropic | ProviderDriverKind::Ollama => Box::new(OpenAiDriver),
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
