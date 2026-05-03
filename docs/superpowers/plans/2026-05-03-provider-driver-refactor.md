# Provider Driver Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove external provider selection, make `--model` / `AEMEATH_MODEL` resolve config model sources, and route LLM calls through explicit `api` drivers.

**Architecture:** Core owns model-source resolution and `ApiDriverKind`. CLI resolves one `ResolvedModel` from `--model` / env / config default, then passes its source config and model entry to LLM. LLM keeps a shared OpenAI Chat provider/transport and moves OpenAI/Zhipu/LiteLLM differences into a `ChatApiDriver` trait.

**Tech Stack:** Rust workspace, clap, serde, reqwest, async_trait, ratatui TUI, cargo test/check.

---

## File Structure

- Modify `aemeath-core/src/provider.rs`
  - Rename API protocol enum to `ApiDriverKind`.
  - Remove `OpenAICompatible` variant.
  - Parse only `anthropic`, `openai`, `zhipu`, `litellm`.
- Modify `aemeath-core/src/config/models.rs`
  - Keep JSON field `models.providers` unchanged.
  - Add `ResolvedModel` and `ModelResolveError`.
  - Add `ModelsConfig::resolve_model_selection` and `ModelsConfig::resolve_default_model`.
  - Keep existing `find_model` as compatibility helper unless all call sites are removed.
- Modify `aemeath-cli/src/cli.rs`
  - Remove `RunArgs.provider` and `Args.provider`.
  - Keep `RunArgs.model` with `env = "AEMEATH_MODEL"`.
  - Add parser tests proving `--provider` is rejected.
- Modify `aemeath-cli/src/main.rs`
  - Replace provider/model defaulting block with `ResolvedModel` resolution.
  - Use `resolved.source_config.api`, `resolved.source_key`, and `resolved.model.id`.
  - Remove `determine_api_type` helper.
- Modify `aemeath-llm/src/client.rs`
  - Replace `ApiType` usage with `ApiDriverKind`.
  - Replace `OpenAIProviderConfig::from_provider_name` with `OpenAIProviderConfig::from_api_driver`.
  - Pass source key only for display/logging, never for driver inference.
- Modify `aemeath-llm/src/providers/openai_compatible/mod.rs`
  - Add `ChatApiDriver` trait and concrete drivers.
  - Store `Box<dyn ChatApiDriver + Send + Sync>` or a cloneable enum-backed driver.
  - Use driver for endpoint suffix and request-body reasoning patch.
- Modify `aemeath-llm/src/providers/openai_compatible/non_stream.rs`
  - Keep calling `build_chat_request_body`; no direct driver logic.
- Modify `aemeath-llm/src/providers/openai_compatible/stream.rs`
  - No planned behavior change; keep parser unchanged.
- Modify `aemeath-cli/src/tui/app/slash.rs`
  - Switch model action should use `api` directly to build `ApiDriverKind` and `OpenAIProviderConfig::from_api_driver`.
  - Avoid source-key based API inference.
- Modify `aemeath-core/src/command/commands/model.rs`
  - Ensure `/model list` displays `<source>/<model>`.
  - Ensure `/model <source>/<model>` supports LiteLLM model IDs containing `/` by splitting once.
- Modify `docs/feature/active.md`
  - Mark #19 and #20 as 实现中.
- Modify `/Users/guoyuqi/.aemeath/config.json`
  - Ensure `models.default` and selected model strings use `<source>/<model>`.
  - Ensure API values are only `anthropic`, `openai`, `zhipu`, `litellm`.

---

### Task 1: Core API Driver Kind

**Files:**
- Modify: `aemeath-core/src/provider.rs`

- [ ] **Step 1: Write failing tests for supported API strings**

Replace the tests module in `aemeath-core/src/provider.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_openai() {
        assert_eq!(ApiDriverKind::from_str("openai"), Some(ApiDriverKind::OpenAI));
    }

    #[test]
    fn test_from_str_zhipu() {
        assert_eq!(ApiDriverKind::from_str("zhipu"), Some(ApiDriverKind::Zhipu));
    }

    #[test]
    fn test_from_str_litellm() {
        assert_eq!(ApiDriverKind::from_str("litellm"), Some(ApiDriverKind::LiteLLM));
    }

    #[test]
    fn test_from_str_rejects_openai_compatible() {
        assert_eq!(ApiDriverKind::from_str("openai-compatible"), None);
        assert_eq!(ApiDriverKind::from_str("openai-completions"), None);
    }

    #[test]
    fn test_as_str_openai() {
        assert_eq!(ApiDriverKind::OpenAI.as_str(), "openai");
    }

    #[test]
    fn test_as_str_anthropic() {
        assert_eq!(ApiDriverKind::Anthropic.as_str(), "anthropic");
    }

    #[test]
    fn test_as_str_zhipu() {
        assert_eq!(ApiDriverKind::Zhipu.as_str(), "zhipu");
    }

    #[test]
    fn test_as_str_litellm() {
        assert_eq!(ApiDriverKind::LiteLLM.as_str(), "litellm");
    }
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test -p aemeath-core provider::tests
```

Expected: FAIL because `ApiDriverKind` does not exist or `openai-compatible` is still accepted.

- [ ] **Step 3: Implement `ApiDriverKind`**

Replace `ApiType` definition and impl in `aemeath-core/src/provider.rs` with:

```rust
//! LLM API driver kinds - core definitions for API protocol selection.
//!
//! The canonical model source list lives in config.json (`models.providers`).
//! This module only defines API driver types understood by code.

use serde::{Deserialize, Serialize};

/// API driver kind. Every model source in config.json maps to one of these via its `api` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiDriverKind {
    Anthropic,
    OpenAI,
    Zhipu,
    LiteLLM,
}

impl Default for ApiDriverKind {
    fn default() -> Self {
        ApiDriverKind::Anthropic
    }
}

impl ApiDriverKind {
    /// Parse from a config string.
    pub fn from_str(s: &str) -> Option<ApiDriverKind> {
        match s {
            "anthropic" => Some(ApiDriverKind::Anthropic),
            "openai" => Some(ApiDriverKind::OpenAI),
            "zhipu" => Some(ApiDriverKind::Zhipu),
            "litellm" => Some(ApiDriverKind::LiteLLM),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ApiDriverKind::Anthropic => "anthropic",
            ApiDriverKind::OpenAI => "openai",
            ApiDriverKind::Zhipu => "zhipu",
            ApiDriverKind::LiteLLM => "litellm",
        }
    }
}
```

Then append the tests from Step 1 below the impl.

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cargo test -p aemeath-core provider::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add aemeath-core/src/provider.rs
git commit -m "refactor(core): replace api type with driver kind"
```

---

### Task 2: Model Source Resolution

**Files:**
- Modify: `aemeath-core/src/config/models.rs`

- [ ] **Step 1: Add failing tests for resolved model selection**

Append these tests to the existing `#[cfg(test)] mod tests` in `aemeath-core/src/config/models.rs`:

```rust
    fn resolver_config() -> ModelsConfig {
        let mut providers = HashMap::new();
        providers.insert(
            "Zhipu".to_string(),
            ProviderModelsConfig {
                base_url: "https://zhipu.example.com".to_string(),
                api_key: "zhipu-key".to_string(),
                api: "zhipu".to_string(),
                models: vec![ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    name: "GLM 5.1".to_string(),
                    context_window: 128_000,
                    max_tokens: 32_000,
                    reasoning: Some(ReasoningConfig::Enabled(true)),
                    ..Default::default()
                }],
            },
        );
        providers.insert(
            "LiteLLM".to_string(),
            ProviderModelsConfig {
                base_url: "https://litellm.example.com".to_string(),
                api_key: "litellm-key".to_string(),
                api: "litellm".to_string(),
                models: vec![ModelEntryConfig {
                    id: "anthropic/claude-opus-4-7".to_string(),
                    name: "Claude via LiteLLM".to_string(),
                    context_window: 200_000,
                    max_tokens: 16_000,
                    reasoning: None,
                    ..Default::default()
                }],
            },
        );
        ModelsConfig {
            mode: String::new(),
            default: "Zhipu/glm-5.1".to_string(),
            providers,
            guidance: HashMap::new(),
        }
    }

    #[test]
    fn test_resolve_model_selection_zhipu() {
        let config = resolver_config();
        let resolved = config.resolve_model_selection("zhipu/glm-5.1").unwrap();
        assert_eq!(resolved.source_key, "Zhipu");
        assert_eq!(resolved.model.id, "glm-5.1");
        assert_eq!(resolved.api, crate::provider::ApiDriverKind::Zhipu);
        assert_eq!(resolved.source_config.api, "zhipu");
    }

    #[test]
    fn test_resolve_model_selection_litellm_model_id_with_slash() {
        let config = resolver_config();
        let resolved = config
            .resolve_model_selection("LiteLLM/anthropic/claude-opus-4-7")
            .unwrap();
        assert_eq!(resolved.source_key, "LiteLLM");
        assert_eq!(resolved.model.id, "anthropic/claude-opus-4-7");
        assert_eq!(resolved.api, crate::provider::ApiDriverKind::LiteLLM);
    }

    #[test]
    fn test_resolve_model_selection_unknown_source_lists_available() {
        let config = resolver_config();
        let err = config.resolve_model_selection("Missing/glm-5.1").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("未找到模型来源 'Missing'"));
        assert!(message.contains("Zhipu"));
        assert!(message.contains("LiteLLM"));
    }

    #[test]
    fn test_resolve_model_selection_unknown_model_lists_available() {
        let config = resolver_config();
        let err = config.resolve_model_selection("Zhipu/glm-x").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("来源 'Zhipu' 中未找到模型 'glm-x'"));
        assert!(message.contains("glm-5.1"));
    }

    #[test]
    fn test_resolve_default_model_uses_config_default() {
        let config = resolver_config();
        let resolved = config.resolve_default_model().unwrap();
        assert_eq!(resolved.source_key, "Zhipu");
        assert_eq!(resolved.model.id, "glm-5.1");
    }
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test -p aemeath-core config::models::tests::test_resolve
```

Expected: FAIL because `ResolvedModel`, `ModelResolveError`, and resolver methods do not exist.

- [ ] **Step 3: Add resolver types and methods**

Add this near the top of `aemeath-core/src/config/models.rs`, after imports:

```rust
use crate::provider::ApiDriverKind;
use std::fmt;
```

Add these types after `ModelsConfig`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    pub source_key: String,
    pub source_config: ProviderModelsConfig,
    pub model: ModelEntryConfig,
    pub api: ApiDriverKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelResolveError {
    MissingSelection { available_sources: Vec<String> },
    InvalidFormat { selection: String },
    UnknownSource { source: String, available_sources: Vec<String> },
    UnknownModel { source: String, query: String, available_models: Vec<String> },
    UnknownApi { source: String, api: String },
}

impl fmt::Display for ModelResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSelection { available_sources } => write!(
                f,
                "未指定模型。请使用 --model <来源>/<模型>。可用来源：\n  {}",
                available_sources.join("\n  ")
            ),
            Self::InvalidFormat { selection } => write!(
                f,
                "模型选择 '{}' 格式无效，请使用 <来源>/<模型>",
                selection
            ),
            Self::UnknownSource { source, available_sources } => write!(
                f,
                "未找到模型来源 '{}'。\n可用来源：\n  {}",
                source,
                available_sources.join("\n  ")
            ),
            Self::UnknownModel { source, query, available_models } => write!(
                f,
                "来源 '{}' 中未找到模型 '{}'。\n可用模型：\n  {}",
                source,
                query,
                available_models.join("\n  ")
            ),
            Self::UnknownApi { source, api } => write!(
                f,
                "来源 '{}' 的 api '{}' 不受支持。支持的 api：anthropic, openai, zhipu, litellm",
                source,
                api
            ),
        }
    }
}

impl std::error::Error for ModelResolveError {}
```

Add these helper methods inside `impl ModelsConfig`:

```rust
    pub fn resolve_model_selection(&self, selection: &str) -> Result<ResolvedModel, ModelResolveError> {
        let (source_query, model_query) = selection
            .split_once('/')
            .ok_or_else(|| ModelResolveError::InvalidFormat {
                selection: selection.to_string(),
            })?;
        if source_query.is_empty() || model_query.is_empty() {
            return Err(ModelResolveError::InvalidFormat {
                selection: selection.to_string(),
            });
        }

        let available_sources = self.available_source_keys();
        let (source_key, source_config) = self
            .providers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(source_query))
            .ok_or_else(|| ModelResolveError::UnknownSource {
                source: source_query.to_string(),
                available_sources: available_sources.clone(),
            })?;

        let model = source_config
            .models
            .iter()
            .find(|m| m.name == model_query)
            .or_else(|| source_config.models.iter().find(|m| m.id == model_query))
            .or_else(|| {
                let norm = normalize_model_key(model_query);
                source_config.models.iter().find(|m| normalize_model_key(&m.name) == norm)
            })
            .or_else(|| {
                let norm = normalize_model_key(model_query);
                source_config.models.iter().find(|m| normalize_model_key(&m.id) == norm)
            })
            .cloned()
            .ok_or_else(|| ModelResolveError::UnknownModel {
                source: source_key.clone(),
                query: model_query.to_string(),
                available_models: source_config
                    .models
                    .iter()
                    .map(|m| {
                        if m.name.is_empty() || m.name == m.id {
                            m.id.clone()
                        } else {
                            format!("{} (id: {})", m.name, m.id)
                        }
                    })
                    .collect(),
            })?;

        let api = ApiDriverKind::from_str(source_config.api.as_str()).ok_or_else(|| {
            ModelResolveError::UnknownApi {
                source: source_key.clone(),
                api: source_config.api.clone(),
            }
        })?;

        Ok(ResolvedModel {
            source_key: source_key.clone(),
            source_config: source_config.clone(),
            model,
            api,
        })
    }

    pub fn resolve_default_model(&self) -> Result<ResolvedModel, ModelResolveError> {
        if !self.default.is_empty() {
            return self.resolve_model_selection(&self.default);
        }

        let mut candidates = self
            .providers
            .iter()
            .filter_map(|(source_key, source_config)| {
                source_config
                    .models
                    .first()
                    .map(|model| (source_key, source_config, model))
            });
        let first = candidates.next();
        if let Some((source_key, source_config, model)) = first {
            if candidates.next().is_none() && source_config.models.len() == 1 {
                let api = ApiDriverKind::from_str(source_config.api.as_str()).ok_or_else(|| {
                    ModelResolveError::UnknownApi {
                        source: source_key.clone(),
                        api: source_config.api.clone(),
                    }
                })?;
                return Ok(ResolvedModel {
                    source_key: source_key.clone(),
                    source_config: source_config.clone(),
                    model: model.clone(),
                    api,
                });
            }
        }

        Err(ModelResolveError::MissingSelection {
            available_sources: self.available_source_keys(),
        })
    }

    pub fn available_source_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.providers.keys().cloned().collect();
        keys.sort();
        keys
    }
```

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cargo test -p aemeath-core config::models::tests::test_resolve
```

Expected: PASS.

- [ ] **Step 5: Run all core config/model tests**

Run:

```bash
cargo test -p aemeath-core config::models::tests provider::tests
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add aemeath-core/src/config/models.rs
git commit -m "feat(core): resolve model sources by selection"
```

---

### Task 3: CLI Removes Provider Argument

**Files:**
- Modify: `aemeath-cli/src/cli.rs`

- [ ] **Step 1: Add failing parser tests**

Append this test module to `aemeath-cli/src/cli.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_cli_rejects_provider_argument() {
        let result = Cli::try_parse_from(["aemeath", "--provider", "Zhipu"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_accepts_model_selection() {
        let cli = Cli::try_parse_from(["aemeath", "--model", "Zhipu/glm-5.1"]).unwrap();
        assert_eq!(cli.run_args.model.as_deref(), Some("Zhipu/glm-5.1"));
    }

    #[test]
    fn test_args_from_run_args_has_no_provider_field_requirement() {
        let cli = Cli::try_parse_from(["aemeath", "--model", "LiteLLM/anthropic/claude-opus-4-7"])
            .unwrap();
        let args: Args = cli.run_args.into();
        assert_eq!(args.model.as_deref(), Some("LiteLLM/anthropic/claude-opus-4-7"));
    }
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test -p aemeath-cli cli::tests
```

Expected: FAIL because `--provider` is still accepted.

- [ ] **Step 3: Remove provider from CLI structs**

In `RunArgs`, delete:

```rust
    /// LLM provider to use (anthropic, openai, openrouter, deepseek, moonshot, zhipu, dashscope, minimax, ollama, openai-compatible)
    #[arg(long, env = "AEMEATH_PROVIDER", default_value = "anthropic")]
    pub provider: String,
```

In `Args`, delete:

```rust
    pub provider: String,
```

In `impl From<RunArgs> for Args`, delete:

```rust
              provider: r.provider,
```

Update the model doc comment to:

```rust
    /// Model selection in <source>/<model> format (overrides AEMEATH_MODEL)
    #[arg(long, env = "AEMEATH_MODEL")]
    pub model: Option<String>,
```

- [ ] **Step 4: Run CLI tests**

Run:

```bash
cargo test -p aemeath-cli cli::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add aemeath-cli/src/cli.rs
git commit -m "refactor(cli): remove provider argument"
```

---

### Task 4: LLM Chat API Driver Strategy

**Files:**
- Modify: `aemeath-llm/src/client.rs`
- Modify: `aemeath-llm/src/providers/openai_compatible/mod.rs`

- [ ] **Step 1: Add failing tests for driver configs and request bodies**

Replace the existing `#[cfg(test)] mod tests` in `aemeath-llm/src/providers/openai_compatible/mod.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use aemeath_core::config::models::{ReasoningConfig, ReasoningOptions};
    use aemeath_core::provider::ApiDriverKind;

    fn provider(
        api: ApiDriverKind,
        reasoning: Option<ReasoningConfig>,
        default_reasoning_enabled: bool,
    ) -> OpenAICompatibleProvider {
        OpenAICompatibleProvider::new(
            OpenAIProviderConfig::from_api_driver(api, "Source"),
            "test-key".to_string(),
            Some("https://example.com".to_string()),
            Some("test-model".to_string()),
            1024,
            default_reasoning_enabled,
            reasoning,
        )
    }

    #[test]
    fn test_build_chat_request_body_openai_object_reasoning() {
        let request_body = provider(
            ApiDriverKind::OpenAI,
            Some(ReasoningConfig::Options(ReasoningOptions {
                effort: Some("medium".to_string()),
            })),
            true,
        )
        .build_chat_request_body(vec![], vec![], true);

        assert_eq!(request_body["reasoning"], serde_json::json!({"effort": "medium"}));
        assert!(request_body.get("thinking").is_none());
        assert!(request_body.get("enable_thinking").is_none());
    }

    #[test]
    fn test_build_chat_request_body_openai_bool_reasoning_omits_reasoning() {
        let request_body = provider(
            ApiDriverKind::OpenAI,
            Some(ReasoningConfig::Enabled(true)),
            true,
        )
        .build_chat_request_body(vec![], vec![], true);

        assert!(request_body.get("reasoning").is_none());
        assert!(request_body.get("thinking").is_none());
    }

    #[test]
    fn test_build_chat_request_body_zhipu_bool_reasoning_sets_thinking_enabled() {
        let request_body = provider(
            ApiDriverKind::Zhipu,
            Some(ReasoningConfig::Enabled(true)),
            true,
        )
        .build_chat_request_body(vec![], vec![], true);

        assert_eq!(request_body["thinking"], serde_json::json!({"type": "enabled"}));
        assert!(request_body.get("reasoning").is_none());
    }

    #[test]
    fn test_build_chat_request_body_zhipu_disabled_sets_thinking_disabled() {
        let request_body = provider(
            ApiDriverKind::Zhipu,
            Some(ReasoningConfig::Enabled(false)),
            true,
        )
        .build_chat_request_body(vec![], vec![], false);

        assert_eq!(request_body["thinking"], serde_json::json!({"type": "disabled"}));
        assert_eq!(request_body["stream"], serde_json::json!(false));
    }

    #[test]
    fn test_build_chat_request_body_litellm_object_reasoning_passthrough() {
        let request_body = provider(
            ApiDriverKind::LiteLLM,
            Some(ReasoningConfig::Options(ReasoningOptions {
                effort: Some("high".to_string()),
            })),
            true,
        )
        .build_chat_request_body(vec![], vec![], true);

        assert_eq!(request_body["reasoning"], serde_json::json!({"effort": "high"}));
        assert!(request_body.get("thinking").is_none());
    }

    #[test]
    fn test_build_chat_request_body_litellm_bool_reasoning_omits_reasoning() {
        let request_body = provider(
            ApiDriverKind::LiteLLM,
            Some(ReasoningConfig::Enabled(true)),
            true,
        )
        .build_chat_request_body(vec![], vec![], true);

        assert!(request_body.get("reasoning").is_none());
        assert!(request_body.get("thinking").is_none());
    }

    #[test]
    fn test_openai_provider_config_from_api_driver() {
        let openai = OpenAIProviderConfig::from_api_driver(ApiDriverKind::OpenAI, "CompanyProxy");
        assert_eq!(openai.source_key, "CompanyProxy");
        assert_eq!(openai.chat_api_suffix, "/v1/chat/completions");
        assert_eq!(openai.api, ApiDriverKind::OpenAI);

        let zhipu = OpenAIProviderConfig::from_api_driver(ApiDriverKind::Zhipu, "MyGLM");
        assert_eq!(zhipu.source_key, "MyGLM");
        assert_eq!(zhipu.chat_api_suffix, "/chat/completions");
        assert_eq!(zhipu.api, ApiDriverKind::Zhipu);
    }
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test -p aemeath-llm providers::openai_compatible::tests
```

Expected: FAIL because `OpenAIProviderConfig::from_api_driver`, `source_key`, and reasoning config parameter do not exist.

- [ ] **Step 3: Update `OpenAIProviderConfig`**

In `aemeath-llm/src/client.rs`, replace the `OpenAIProviderConfig` struct with:

```rust
#[derive(Debug, Clone)]
pub struct OpenAIProviderConfig {
    pub source_key: String,
    pub api: ApiDriverKind,
    pub chat_api_suffix: String,
}

impl OpenAIProviderConfig {
    pub fn from_api_driver(api: ApiDriverKind, source_key: impl Into<String>) -> Self {
        let chat_api_suffix = match api {
            ApiDriverKind::Zhipu => "/chat/completions",
            ApiDriverKind::OpenAI | ApiDriverKind::LiteLLM => "/v1/chat/completions",
            ApiDriverKind::Anthropic => "/v1/messages",
        };
        Self {
            source_key: source_key.into(),
            api,
            chat_api_suffix: chat_api_suffix.to_string(),
        }
    }
}
```

Also update imports in `client.rs`:

```rust
use aemeath_core::provider::ApiDriverKind;
```

- [ ] **Step 4: Add `ChatApiDriver` and concrete drivers**

In `aemeath-llm/src/providers/openai_compatible/mod.rs`, add after imports:

```rust
use aemeath_core::config::models::ReasoningConfig;
use aemeath_core::provider::ApiDriverKind;
```

Add before `pub struct OpenAICompatibleProvider`:

```rust
trait ChatApiDriver: Send + Sync {
    fn apply_reasoning(
        &self,
        request_body: &mut serde_json::Value,
        reasoning: Option<&ReasoningConfig>,
        default_reasoning_enabled: bool,
    );
}

struct OpenAiDriver;
struct ZhipuDriver;
struct LiteLlmDriver;

impl ChatApiDriver for OpenAiDriver {
    fn apply_reasoning(
        &self,
        request_body: &mut serde_json::Value,
        reasoning: Option<&ReasoningConfig>,
        _default_reasoning_enabled: bool,
    ) {
        if let Some(effort) = reasoning.and_then(|r| r.effort()) {
            request_body["reasoning"] = serde_json::json!({ "effort": effort });
        }
    }
}

impl ChatApiDriver for ZhipuDriver {
    fn apply_reasoning(
        &self,
        request_body: &mut serde_json::Value,
        reasoning: Option<&ReasoningConfig>,
        default_reasoning_enabled: bool,
    ) {
        let enabled = reasoning
            .and_then(|r| r.enabled())
            .unwrap_or(default_reasoning_enabled);
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({ "type": thinking_type });
    }
}

impl ChatApiDriver for LiteLlmDriver {
    fn apply_reasoning(
        &self,
        request_body: &mut serde_json::Value,
        reasoning: Option<&ReasoningConfig>,
        _default_reasoning_enabled: bool,
    ) {
        if let Some(effort) = reasoning.and_then(|r| r.effort()) {
            request_body["reasoning"] = serde_json::json!({ "effort": effort });
        }
    }
}

fn chat_driver_for(api: ApiDriverKind) -> Box<dyn ChatApiDriver> {
    match api {
        ApiDriverKind::OpenAI => Box::new(OpenAiDriver),
        ApiDriverKind::Zhipu => Box::new(ZhipuDriver),
        ApiDriverKind::LiteLLM => Box::new(LiteLlmDriver),
        ApiDriverKind::Anthropic => Box::new(OpenAiDriver),
    }
}
```

- [ ] **Step 5: Update `OpenAICompatibleProvider` fields and constructor**

In `OpenAICompatibleProvider`, replace:

```rust
      reasoning: std::sync::Arc<std::sync::atomic::AtomicBool>,
      openai_reasoning_effort: std::sync::Arc<std::sync::Mutex<Option<String>>>,
```

with:

```rust
      reasoning: std::sync::Arc<std::sync::atomic::AtomicBool>,
      reasoning_config: std::sync::Arc<std::sync::Mutex<Option<ReasoningConfig>>>,
      driver: Box<dyn ChatApiDriver>,
```

Change `new` signature to:

```rust
      pub fn new(
          config: OpenAIProviderConfig,
          api_key: String,
          base_url: Option<String>,
          model: Option<String>,
          max_tokens: u32,
          reasoning: bool,
          reasoning_config: Option<ReasoningConfig>,
      ) -> Self {
```

Inside constructor, before `Self {`, add:

```rust
          let driver = chat_driver_for(config.api);
```

Set fields:

```rust
              reasoning: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(reasoning)),
              reasoning_config: std::sync::Arc::new(std::sync::Mutex::new(reasoning_config)),
              driver,
```

- [ ] **Step 6: Replace `apply_reasoning_fields` implementation**

Replace `apply_reasoning_fields` with:

```rust
    fn apply_reasoning_fields(&self, request_body: &mut serde_json::Value) {
        let default_reasoning_enabled = self.reasoning.load(std::sync::atomic::Ordering::Relaxed);
        let reasoning = self.reasoning_config.lock().ok().and_then(|guard| guard.clone());
        self.driver
            .apply_reasoning(request_body, reasoning.as_ref(), default_reasoning_enabled);
    }
```

Replace `reasoning_effort` trait method implementation later in the file with:

```rust
    fn reasoning_effort(&self) -> Option<String> {
        self.reasoning_config
            .lock()
            .ok()
            .and_then(|g| g.as_ref().and_then(|r| r.effort().map(str::to_string)))
    }
```

Replace `set_reasoning_effort` implementation with:

```rust
    fn set_reasoning_effort(&self, effort: Option<String>) {
        if let Ok(mut guard) = self.reasoning_config.lock() {
            *guard = effort.map(|effort| {
                ReasoningConfig::Options(aemeath_core::config::models::ReasoningOptions {
                    effort: Some(effort),
                })
            });
        }
    }
```

- [ ] **Step 7: Update logs to use source key**

In `mod.rs`, replace log field access:

```rust
self.config.provider_name
```

with:

```rust
self.config.source_key
```

- [ ] **Step 8: Update `LlmClient` constructors**

In `aemeath-llm/src/client.rs`, update `LlmClient::with_provider` signature:

```rust
      pub fn with_provider(
          api: ApiDriverKind,
          api_key: String,
          base_url: Option<String>,
          model: Option<String>,
          max_tokens: u32,
          reasoning: bool,
          reasoning_config: Option<aemeath_core::config::models::ReasoningConfig>,
      ) -> Self {
```

Use:

```rust
              ApiDriverKind::Anthropic => Arc::new(crate::providers::AnthropicProvider::new(
                  api_key, base_url, model, max_tokens,
              )),
              ApiDriverKind::OpenAI | ApiDriverKind::Zhipu | ApiDriverKind::LiteLLM => {
                  let config = OpenAIProviderConfig::from_api_driver(api, api.as_str());
                  Arc::new(crate::providers::OpenAICompatibleProvider::new(
                      config,
                      api_key,
                      base_url,
                      model,
                      max_tokens,
                      reasoning,
                      reasoning_config,
                  ))
              }
```

Update `with_openai_config` and `from_config` to accept/pass `reasoning_config`.

`from_config` should become:

```rust
      pub fn from_config(
          api: ApiDriverKind,
          api_key: String,
          base_url: Option<String>,
          model: String,
          max_tokens: u32,
          reasoning: bool,
          reasoning_config: Option<aemeath_core::config::models::ReasoningConfig>,
          openai_config: Option<OpenAIProviderConfig>,
      ) -> Self {
```

- [ ] **Step 9: Run LLM tests**

Run:

```bash
cargo test -p aemeath-llm providers::openai_compatible::tests
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add aemeath-llm/src/client.rs aemeath-llm/src/providers/openai_compatible/mod.rs
git commit -m "refactor(llm): add chat api driver strategy"
```

---

### Task 5: CLI Main Uses ResolvedModel

**Files:**
- Modify: `aemeath-cli/src/main.rs`

- [ ] **Step 1: Add focused tests for selection helper**

Add a pure helper in Step 3, then add these tests at the end of `aemeath-cli/src/main.rs` test module. If a test module already exists, append inside it:

```rust
    #[test]
    fn test_select_model_prefers_cli_model() {
        let cfg = test_config_for_model_selection();
        let selected = select_model_for_run(Some("LiteLLM/anthropic/claude-opus-4-7"), Some(&cfg)).unwrap();
        assert_eq!(selected.source_key, "LiteLLM");
        assert_eq!(selected.model.id, "anthropic/claude-opus-4-7");
    }

    #[test]
    fn test_select_model_uses_config_default() {
        let cfg = test_config_for_model_selection();
        let selected = select_model_for_run(None, Some(&cfg)).unwrap();
        assert_eq!(selected.source_key, "Zhipu");
        assert_eq!(selected.model.id, "glm-5.1");
    }

    #[test]
    fn test_select_model_without_config_errors() {
        let err = select_model_for_run(None, None).unwrap_err();
        assert!(err.contains("未指定模型"));
    }

    fn test_config_for_model_selection() -> aemeath_core::config::Config {
        use aemeath_core::config::{Config, ModelEntryConfig, ModelsConfig, ProviderModelsConfig};
        use std::collections::HashMap;

        let mut providers = HashMap::new();
        providers.insert(
            "Zhipu".to_string(),
            ProviderModelsConfig {
                api: "zhipu".to_string(),
                api_key: "zhipu-key".to_string(),
                base_url: "https://zhipu.example.com".to_string(),
                models: vec![ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    max_tokens: 128000,
                    ..Default::default()
                }],
            },
        );
        providers.insert(
            "LiteLLM".to_string(),
            ProviderModelsConfig {
                api: "litellm".to_string(),
                api_key: "litellm-key".to_string(),
                base_url: "https://litellm.example.com".to_string(),
                models: vec![ModelEntryConfig {
                    id: "anthropic/claude-opus-4-7".to_string(),
                    max_tokens: 16000,
                    ..Default::default()
                }],
            },
        );
        Config {
            models: ModelsConfig {
                default: "Zhipu/glm-5.1".to_string(),
                providers,
                ..Default::default()
            },
            ..Default::default()
        }
    }
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test -p aemeath-cli tests::test_select_model
```

Expected: FAIL because `select_model_for_run` does not exist and `Args.provider` removal broke main.

- [ ] **Step 3: Add `select_model_for_run` helper**

Add near the bottom of `aemeath-cli/src/main.rs`, before tests:

```rust
fn select_model_for_run(
    requested_model: Option<&str>,
    config_file: Option<&aemeath_core::config::Config>,
) -> Result<aemeath_core::config::models::ResolvedModel, String> {
    let cfg = config_file.ok_or_else(|| {
        "未指定模型，且未找到 config.json。请使用 --model <来源>/<模型> 或配置 models.default".to_string()
    })?;

    if let Some(selection) = requested_model.filter(|s| !s.is_empty()) {
        return cfg
            .models
            .resolve_model_selection(selection)
            .map_err(|e| e.to_string());
    }

    cfg.models.resolve_default_model().map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Replace `run_chat` model/provider resolution**

In `run_chat`, delete the entire block from the comment:

```rust
    // 应用 config.json 默认值（当 CLI/env 未指定时）
```

through creation of `current_model_entry` and `reasoning`, replacing it with:

```rust
    let resolved_model = match select_model_for_run(args.model.as_deref(), config_file.as_ref()) {
        Ok(model) => model,
        Err(message) => {
            eprintln!("Error: {}", message);
            std::process::exit(1);
        }
    };

    let api_type = resolved_model.api;

    if args.base_url.is_none() && std::env::var("AEMEATH_BASE_URL").is_err() {
        if !resolved_model.source_config.base_url.is_empty() {
            args.base_url = Some(resolved_model.source_config.base_url.clone());
        }
    }

    let api_key = args.api_key.unwrap_or_else(|| {
        std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_else(|_| {
                if !resolved_model.source_config.api_key.is_empty() {
                    return resolved_model.source_config.api_key.clone();
                }
                eprintln!("Error: API key not set. Use --api-key, set LLM_API_KEY, or configure apiKey in ~/.aemeath/config.json");
                std::process::exit(1);
            })
    });

    let model = resolved_model.model.id.clone();
    let max_tokens = args
        .max_tokens
        .or_else(|| (resolved_model.model.max_tokens > 0).then_some(resolved_model.model.max_tokens))
        .unwrap_or(32000);

    let openai_config = if !matches!(api_type, aemeath_core::provider::ApiDriverKind::Anthropic) {
        Some(OpenAIProviderConfig::from_api_driver(api_type, &resolved_model.source_key))
    } else {
        None
    };

    let reasoning = resolved_model
        .model
        .reasoning
        .as_ref()
        .and_then(|r| r.enabled())
        .unwrap_or(!args.no_think);
```

Then update `openai_reasoning_effort` config source from `current_model_entry` to `resolved_model.model`:

```rust
          .or_else(|| {
              resolved_model
                  .model
                  .reasoning
                  .as_ref()
                  .and_then(|r| r.effort().map(str::to_string))
          })
```

Update `LlmClient::from_config` call to pass:

```rust
          resolved_model.model.reasoning.clone(),
```

before `openai_config`.

Update current model display to:

```rust
let current_model_display = format!("{}/{}", resolved_model.source_key, if resolved_model.model.name.is_empty() { &resolved_model.model.id } else { &resolved_model.model.name });
```

- [ ] **Step 5: Remove `determine_api_type` helper and old tests**

Delete `determine_api_type` and any tests that reference it.

- [ ] **Step 6: Run CLI selection tests**

Run:

```bash
cargo test -p aemeath-cli tests::test_select_model
```

Expected: PASS.

- [ ] **Step 7: Run cargo check to surface call-site errors**

Run:

```bash
cargo check
```

Expected: It may fail in TUI/slash and LLM call sites that still use old signatures. Do not commit until Task 6 fixes downstream call sites.

---

### Task 6: TUI Model Switch and Command Model Compatibility

**Files:**
- Modify: `aemeath-cli/src/tui/app/slash.rs`
- Modify: `aemeath-core/src/command/commands/model.rs`
- Maybe modify: `aemeath-core/src/command/mod.rs` or command action definition file if `SwitchModel` needs fields added.

- [ ] **Step 1: Inspect command action definition and model command**

Read these files before editing:

```bash
# use Read tool, not shell cat
```

Files:

- `aemeath-core/src/command/mod.rs`
- `aemeath-core/src/command/commands/model.rs`

Confirm where `CommandAction::SwitchModel` is defined and where `api_type`, `provider_name`, `model_id`, `reasoning` are populated.

- [ ] **Step 2: Add/adjust tests in model command**

In `aemeath-core/src/command/commands/model.rs`, add tests that execute the model switch parser/handler if existing test helpers support it. If command execution is hard to instantiate, add focused tests for the pure helper used by the command.

Required assertions:

```rust
assert!(displayed_models.contains("Zhipu/glm-5.1"));
assert!(displayed_models.contains("LiteLLM/anthropic/claude-opus-4-7"));
```

And for resolving a switch action:

```rust
assert_eq!(provider_name, "LiteLLM");
assert_eq!(model_id, "anthropic/claude-opus-4-7");
assert_eq!(api_type, "litellm");
```

- [ ] **Step 3: Run model command tests and verify they fail**

Run:

```bash
cargo test -p aemeath-core command::commands::model::tests
```

Expected: FAIL if current command cannot resolve LiteLLM model IDs containing `/` or still assumes provider semantics.

- [ ] **Step 4: Update model command to use `resolve_model_selection`**

In `model.rs`, when user provides `/model <query>`:

- If query contains `/`, call `ctx.config.models.resolve_model_selection(query)`.
- If query does not contain `/`, keep existing fuzzy behavior if present, but final result must be a `ResolvedModel`.
- Populate `SwitchModel` with:
  - `provider_name = resolved.source_key`
  - `model_id = resolved.model.id`
  - `model_name = resolved.model.name`
  - `base_url = resolved.source_config.base_url`
  - `api_key = resolved.source_config.api_key`
  - `api_type = resolved.api.as_str().to_string()`
  - `max_tokens = resolved.model.max_tokens`
  - `context_window = resolved.model.context_window`
  - `reasoning = resolved.model.reasoning.as_ref().and_then(|r| r.enabled())`

- [ ] **Step 5: Update TUI slash switch branch**

In `aemeath-cli/src/tui/app/slash.rs`, replace old `ApiType` usage with:

```rust
let api_type = aemeath_core::provider::ApiDriverKind::from_str(api_type.as_str()).unwrap_or(
    aemeath_core::provider::ApiDriverKind::OpenAI,
);

let openai_config = if !matches!(
    api_type,
    aemeath_core::provider::ApiDriverKind::Anthropic
) {
    Some(aemeath_llm::client::OpenAIProviderConfig::from_api_driver(
        api_type,
        &provider_name,
    ))
} else {
    None
};
```

Update `LlmClient::from_config` call to include a reasoning config argument. Since `SwitchModel` currently only carries bool reasoning, pass:

```rust
reasoning.map(aemeath_core::config::models::ReasoningConfig::Enabled)
```

before `openai_config`.

- [ ] **Step 6: Run tests and cargo check**

Run:

```bash
cargo test -p aemeath-core command::commands::model::tests
cargo check
```

Expected: PASS for model command tests and no compile errors.

- [ ] **Step 7: Commit Tasks 5 and 6 together**

```bash
git add aemeath-cli/src/main.rs aemeath-cli/src/tui/app/slash.rs aemeath-core/src/command/commands/model.rs aemeath-core/src/command/mod.rs
git commit -m "refactor(cli): resolve models from source selections"
```

---

### Task 7: Feature Status and Global Config

**Files:**
- Modify: `docs/feature/active.md`
- Modify: `/Users/guoyuqi/.aemeath/config.json`

- [ ] **Step 1: Update feature statuses**

In `docs/feature/active.md`, update Feature #19 and #20 status text to `实现中` if the table has a status column. If the table does not have a status column, add one line under each feature section:

```markdown
**状态**：实现中
```

- [ ] **Step 2: Update global config**

Read `/Users/guoyuqi/.aemeath/config.json` first. Ensure:

- `models.default` is a full selection like `Zhipu/glm-5.1`.
- Zhipu source has `"api": "zhipu"`.
- LiteLLM source has `"api": "litellm"`.
- No source uses `"api": "openai-compatible"`.

Do not change API keys or unrelated settings.

- [ ] **Step 3: Validate JSON**

Run:

```bash
python3 -m json.tool "$HOME/.aemeath/config.json" >/dev/null
```

Expected: exit 0.

- [ ] **Step 4: Commit feature status only**

Do not commit user global config. Commit repo doc status:

```bash
git add docs/feature/active.md
git commit -m "docs: mark provider driver features in progress"
```

---

### Task 8: Full Verification and Cleanup

**Files:**
- Potentially modify any file touched above for compile/test fixes only.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: no output or formatted files.

- [ ] **Step 2: Core tests**

Run:

```bash
cargo test -p aemeath-core
```

Expected: PASS.

- [ ] **Step 3: LLM tests**

Run:

```bash
cargo test -p aemeath-llm
```

Expected: PASS.

- [ ] **Step 4: CLI tests**

Run:

```bash
cargo test -p aemeath-cli
```

Expected: PASS.

- [ ] **Step 5: Workspace check**

Run:

```bash
cargo check
```

Expected: PASS.

- [ ] **Step 6: Search for removed terms**

Use Grep tool, not shell grep, to check:

Patterns:

```text
OpenAICompatible
AEMEATH_PROVIDER
--provider
from_provider_name
```

Expected:

- `OpenAICompatible` may remain only in old type/file names if not renamed, but must not exist as an API enum variant.
- `AEMEATH_PROVIDER` must not appear in CLI parsing or model resolution code.
- `--provider` must not appear in user-facing CLI docs/help strings.
- `from_provider_name` must be removed.

- [ ] **Step 7: Final commit if verification fixes were needed**

If Step 1-6 required fixes:

```bash
git add <fixed-files>
git commit -m "fix: complete provider driver refactor verification"
```

If no fixes were needed, do not create an empty commit.

---

## Plan Self-Review

Spec coverage:

- Removes `--provider` and `AEMEATH_PROVIDER`: Task 3, Task 5, Task 8.
- Makes `--model` / `AEMEATH_MODEL` full model selection: Task 2, Task 3, Task 5.
- Treats `models.providers` key as source key: Task 2, Task 5, Task 6.
- Uses `api` as code driver: Task 1, Task 2, Task 4, Task 5, Task 6.
- Removes `openai-compatible` API kind: Task 1, Task 8.
- Adds `ResolvedModel`: Task 2.
- Adds `ChatApiDriver`: Task 4.
- Implements reasoning behavior for OpenAI/Zhipu/LiteLLM: Task 4.
- Updates TUI `/model`: Task 6.
- Updates feature status and config: Task 7.
- Runs required verification: Task 8.

Placeholder scan: no TBD/TODO placeholders. Task 6 contains an inspection step because exact command internals must be read before safe edits; it also gives concrete required assertions and field mapping.

Type consistency: `ApiDriverKind`, `ResolvedModel`, `ModelResolveError`, `OpenAIProviderConfig::from_api_driver`, and `ReasoningConfig` names are consistent across tasks.
