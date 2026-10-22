//! Chat Provider driver 抽象：不同供应商的推理字段差异化处理

use crate::domain::capability::{
    maximum_supported, normalize_levels, resolve_supported, ReasoningLevel,
};
use crate::ProviderDriverKind;

use super::ReasoningConfig;

fn effort_capability(maximum: ReasoningLevel) -> Vec<ReasoningLevel> {
    normalize_levels(
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
    )
}

fn toggle_capability(on_level: ReasoningLevel) -> Vec<ReasoningLevel> {
    normalize_levels([ReasoningLevel::Off, on_level])
}

/// GLM 系列（智谱官方「深度思考」文档 Coding Plan 语义；方舟上的 GLM
/// 暂按同源处理，#2032）：ZhipuDriver 与 VolcengineDriver 共用。
/// - `glm-5.3*`（强制思考）：none/minimal/low → low；medium/high → high；
///   xhigh/max → max。
/// - `glm-5.2`：none/minimal → 放弃思考（off）；low/medium → high；
///   xhigh → max。
/// - 其余模型恒等。
fn glm_server_merge(model: &str, level: ReasoningLevel) -> ReasoningLevel {
    use ReasoningLevel::*;
    if model.starts_with("glm-5.3") {
        match level {
            Off | Minimal | Low => Low,
            Medium | High => High,
            Xhigh | Max => Max,
        }
    } else if model.starts_with("glm-5.2") {
        match level {
            Off | Minimal => Off,
            Low | Medium => High,
            High => High,
            Xhigh | Max => Max,
        }
    } else {
        level
    }
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

    /// 对 ReasoningLevel 做 capability resolve + wire_effort 映射，一步到位。
    ///
    /// 等价于 `self.wire_effort(self.resolve_capability(level))`，
    /// 但避免 `clamp_effort` 所需的 `as_str() → parse()` 字符串往返。
    fn resolve_effort(&self, level: ReasoningLevel) -> &'static str {
        self.wire_effort(resolve_supported(&self.reasoning_capability(), level))
    }

    /// 将测试中的 legacy effort 字符串投影到 capability 允许的 wire 档位。
    #[cfg(test)]
    fn clamp_effort<'a>(&self, effort: &'a str) -> &'a str {
        ReasoningLevel::parse(effort)
            .map(|requested| {
                self.wire_effort(resolve_supported(&self.reasoning_capability(), requested))
            })
            .unwrap_or(effort)
    }

    /// 支持档位阶梯（升序去重；#1861 v4 摊平为 `ModelInfo.supported_reasoning` 同构）。
    fn reasoning_capability(&self) -> Vec<ReasoningLevel>;

    fn max_reasoning_level(&self) -> ReasoningLevel {
        maximum_supported(&self.reasoning_capability())
    }

    /// 服务端对请求档位的归并行为（#2032）：driver 是协议形状唯一知识源，
    /// 返回服务端实际执行的档位（UI 映射表、/think 回执与 wire effort 的
    /// 统一显示/发送口径）。默认恒等（服务端忠实执行）。
    ///
    /// 知识来源 MUST 为各 provider 官方文档，NEVER 依赖传闻或旧注释。
    fn server_effective_effort(&self, _model: &str, level: ReasoningLevel) -> ReasoningLevel {
        level
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

/// Kimi（platform.kimi.com / api.kimi.com）#2032 收口：用户曾以 openai
/// driver 兜底，kimi-k3 的 effort 词汇与归并无处安放。
#[derive(Debug)]
pub struct KimiDriver;

impl ChatApiDriver for OpenAiDriver {
    /// OpenAI 服务端归并（#2032，真相源：developers.openai.com
    /// 「Reasoning models」）：词汇全集 none..max；`gpt-6.1*` 不支持
    /// none/minimal（官方默认 medium）；`gpt-6-astra` 不支持 none
    /// （官方未给映射，暂恒等由 400 暴露）。其余模型恒等。
    fn server_effective_effort(&self, model: &str, level: ReasoningLevel) -> ReasoningLevel {
        use ReasoningLevel::*;
        if model.starts_with("gpt-6.1") {
            match level {
                Off | Minimal => Medium,
                other => other,
            }
        } else {
            level
        }
    }
    fn reasoning_capability(&self) -> Vec<ReasoningLevel> {
        normalize_levels([
            ReasoningLevel::Off,
            ReasoningLevel::Minimal,
            ReasoningLevel::Low,
            ReasoningLevel::Medium,
            ReasoningLevel::High,
            ReasoningLevel::Xhigh,
            ReasoningLevel::Max,
        ])
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
        // OpenAI Chat Completions 正式协议只接受顶层 `reasoning_effort`（#973）；
        // 嵌套 `reasoning` 是 Responses 风格，部分网关（如 OmniRoute）不会为它
        // 注入 reasoning summary。只提取 effort 编码为顶层字段，NEVER 双发。
        if let Some(effort) = reasoning_config.and_then(|config| config.as_effort()) {
            request_body["reasoning_effort"] = serde_json::Value::String(effort);
        }
    }
}

impl ChatApiDriver for ZhipuDriver {
    fn reasoning_capability(&self) -> Vec<ReasoningLevel> {
        effort_capability(ReasoningLevel::Max)
    }

    /// 智谱服务端 effort 归并（#2032，真相源：智谱开放平台「深度思考」
    /// 官方文档 Coding Plan 请求语义）：
    /// - `glm-5.3`/`glm-5.3-flash`/`glm-5.3-flashx`（强制思考，不支持
    ///   disabled）：`none`/`minimal`/`low` → `low`；`medium`/`high` →
    ///   `high`；`xhigh`/`max` → `max`。
    /// - `glm-5.2`：`none`/`minimal` → 放弃思考（off）；`low`/`medium` →
    ///   `high`；`xhigh` → `max`。
    /// - 其余模型（不支持 reasoning_effort 字段，thinking.type 动态思考）：
    ///   恒等，无档位语义。
    fn server_effective_effort(&self, model: &str, level: ReasoningLevel) -> ReasoningLevel {
        glm_server_merge(model, level)
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

        // thinking 开启时附带 reasoning_effort 字段；effort 值由会话链的
        // server_effective_effort 归并后经 for_scope/from_scope 写入，
        // 此处只透传（NEVER 在此层再做映射）。
        if enabled {
            if let Some(effort) = reasoning_config.and_then(|c| c.as_effort()) {
                request_body["reasoning_effort"] = serde_json::Value::String(effort);
            }
        }
    }
}

impl ChatApiDriver for LiteLlmDriver {
    fn reasoning_capability(&self) -> Vec<ReasoningLevel> {
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
    fn reasoning_capability(&self) -> Vec<ReasoningLevel> {
        // 方舟承载 GLM/Kimi-K3 等 effort 型模型（#2032 修正：原 Medium 上限
        // 过保守导致用户借道 zhipu driver）；开关型 doubao 对多发的
        // reasoning_effort 字段按 OpenAI 兼容容错忽略。
        effort_capability(ReasoningLevel::Max)
    }

    /// 方舟上的 GLM 系列暂按智谱官方归并同源处理（#2032 用户拍板；
    /// 方舟文档未直接公开 effort 归并规则）；其余模型恒等。
    fn server_effective_effort(&self, model: &str, level: ReasoningLevel) -> ReasoningLevel {
        glm_server_merge(model, level)
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
        // 方舟与智谱同构：thinking.type 开关 + reasoning_effort 透传
        //（effort 值由会话链 server_effective_effort 归并后写入）。
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            _ => reasoning_enabled,
        };
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({"type": thinking_type});
        if enabled {
            if let Some(effort) = reasoning_config.and_then(|c| c.as_effort()) {
                request_body["reasoning_effort"] = serde_json::Value::String(effort);
            }
        }
    }
}

impl ChatApiDriver for MinimaxDriver {
    fn reasoning_capability(&self) -> Vec<ReasoningLevel> {
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
    fn reasoning_capability(&self) -> Vec<ReasoningLevel> {
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

impl ChatApiDriver for KimiDriver {
    /// Kimi 官方思考模型页：`kimi-k3` 词汇 low/high/max；k2.7-code 始终思考
    /// （disabled 报错）；k2.6 thinking 开关。阶梯按全档授予，词汇收敛由
    /// server_effective_effort 归并。
    fn reasoning_capability(&self) -> Vec<ReasoningLevel> {
        effort_capability(ReasoningLevel::Max)
    }

    /// k3 归并按词汇表分组（minimal/low→low、medium/high→high、
    /// xhigh/max→max；k3 始终推理，off→low）；k2.7 始终思考 off→low；
    /// 其余恒等。词汇表来自官方文档，分组参照同词汇表官方先例（DeepSeek
    /// Thinking Mode 映射表）。
    fn server_effective_effort(&self, model: &str, level: ReasoningLevel) -> ReasoningLevel {
        use ReasoningLevel::*;
        // 兼容官方全名（kimi-k3）与用户自定义短 id（k3）两种形态。
        let is_k3 = model.starts_with("kimi-k3") || model.starts_with("k3");
        let is_k27 = model.starts_with("kimi-k2.7") || model.starts_with("k2.7");
        if is_k3 {
            match level {
                Off | Minimal | Low => Low,
                Medium | High => High,
                Xhigh | Max => Max,
            }
        } else if is_k27 {
            match level {
                Off => Low,
                other => other,
            }
        } else {
            level
        }
    }

    fn wire_effort(&self, level: ReasoningLevel) -> &'static str {
        // k3 词汇无 none；off 由归并层收敛为 low，此处仅兜底。
        match level {
            ReasoningLevel::Off => "low",
            other => other.as_str(),
        }
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        _reasoning_enabled: bool,
    ) {
        // Kimi 与 OpenAI 同构：顶层 reasoning_effort（k2.6/k2.7 不支持该
        // 字段，服务端按 OpenAI 兼容容错忽略；k3 归并层保证 off→low）。
        if let Some(effort) = reasoning_config.and_then(|c| c.as_effort()) {
            request_body["reasoning_effort"] = serde_json::Value::String(effort);
        }
    }
}

impl ChatApiDriver for DeepSeekDriver {
    /// DeepSeek 服务端归并（#2032，真相源：api-docs.deepseek.com
    /// 「Thinking Mode」官方映射表）：minimal → low；medium → high；
    /// xhigh → high；其余恒等（off 支持 disabled 关思考）。
    fn server_effective_effort(&self, _model: &str, level: ReasoningLevel) -> ReasoningLevel {
        use ReasoningLevel::*;
        match level {
            Minimal => Low,
            Medium | Xhigh => High,
            other => other,
        }
    }
    fn reasoning_capability(&self) -> Vec<ReasoningLevel> {
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
    fn reasoning_capability(&self) -> Vec<ReasoningLevel> {
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
        ProviderDriverKind::Kimi => Box::new(KimiDriver),
        ProviderDriverKind::Agnes => Box::new(AgnesDriver),
        // Ollama 有专用 OllamaProvider，不经此 OpenAI 兼容驱动；兜底走 OpenAI 驱动。
        ProviderDriverKind::Anthropic | ProviderDriverKind::Ollama => Box::new(OpenAiDriver),
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
