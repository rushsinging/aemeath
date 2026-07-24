//! ReasoningConfig + per-driver `apply_reasoning_fields` contract tests.
//!
//! 锁定 #1393 的契约：
//! - `ReasoningConfig::ThinkingBudget` 不再生成 `reasoning` / `reasoning_effort`；
//! - `for_scope(level, driver)` 对 `ThinkingBudget` 保留原值；
//! - `clamped(driver)` 对 `ThinkingBudget` 保留原值；
//! - Volcengine / Minimax / Mimo / Agnes 的 toggle 语义未因 #1393 误删。

use super::super::super::client::OpenAIProviderConfig;
use super::super::driver::{
    ChatApiDriver, LiteLlmDriver, MinimaxDriver, OpenAiDriver, VolcengineDriver, ZhipuDriver,
};
use super::super::{OpenAICompatibleProvider, ReasoningConfig};
use super::common::{assert_no_reasoning_fields, base_body};

use crate::domain::invoke::InvocationScope;
use crate::ports::ReasoningLevel;
use crate::ProviderDriverKind;

fn openai_provider_with_reasoning_config(
    driver_kind: ProviderDriverKind,
    source_key: &str,
    reasoning_config: Option<ReasoningConfig>,
) -> OpenAICompatibleProvider {
    let config = OpenAIProviderConfig::from_driver(driver_kind, source_key);
    OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        None,
        Some("test-model".to_string()),
        8192,
        false,
        reasoning_config,
        30,
    )
}

fn scope_with_effective_reasoning(effective: ReasoningLevel) -> InvocationScope {
    InvocationScope::new("test-model", 8192, effective, effective).expect("valid scope")
}

#[test]
fn openai_object_reasoning_sends_reasoning_only() {
    let config = ReasoningConfig::Object(serde_json::json!({"effort":"medium"}));
    let mut body = base_body();

    OpenAiDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(
        body.get("reasoning"),
        Some(&serde_json::json!({"effort":"medium"}))
    );
    assert!(body.get("thinking").is_none());
    assert!(body.get("enable_thinking").is_none());
}

#[test]
fn openai_bool_reasoning_sends_no_reasoning_fields() {
    let config = ReasoningConfig::Bool(true);
    let mut body = base_body();

    OpenAiDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_no_reasoning_fields(&body);
}

#[test]
fn zhipu_bool_true_sends_enabled_thinking() {
    let config = ReasoningConfig::Bool(true);
    let mut body = base_body();

    ZhipuDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"enabled"}))
    );
    assert!(body.get("reasoning").is_none());
}

#[test]
fn zhipu_bool_false_sends_disabled_thinking() {
    let config = ReasoningConfig::Bool(false);
    let mut body = base_body();

    ZhipuDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"disabled"}))
    );
    assert!(body.get("reasoning").is_none());
}

#[test]
fn litellm_object_reasoning_sends_reasoning_effort() {
    let config = ReasoningConfig::Object(serde_json::json!({"effort":"high"}));
    let mut body = base_body();

    LiteLlmDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(
        body.get("reasoning_effort"),
        Some(&serde_json::json!("high"))
    );
    assert!(body.get("reasoning").is_none());
    assert!(body.get("thinking").is_none());
}

#[test]
fn litellm_bool_reasoning_sends_no_reasoning_fields() {
    let config = ReasoningConfig::Bool(true);
    let mut body = base_body();

    LiteLlmDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_no_reasoning_fields(&body);
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn openai_thinking_budget_does_not_generate_effort() {
    let config = ReasoningConfig::ThinkingBudget(4096);
    let mut body = base_body();

    OpenAiDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_no_reasoning_fields(&body);
}

#[test]
fn litellm_thinking_budget_does_not_generate_effort() {
    let config = ReasoningConfig::ThinkingBudget(40000);
    let mut body = base_body();

    LiteLlmDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert!(body.get("reasoning_effort").is_none());
    assert!(body.get("reasoning").is_none());
}

#[test]
fn volcengine_thinking_budget_sends_enabled_thinking() {
    let config = ReasoningConfig::ThinkingBudget(40000);
    let mut body = base_body();

    VolcengineDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    // Volcengine 使用 thinking.type 字段，ThinkingBudget 表示启用。
    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"enabled"}))
    );
    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn volcengine_bool_false_sends_disabled_thinking() {
    let config = ReasoningConfig::Bool(false);
    let mut body = base_body();

    VolcengineDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"disabled"}))
    );
    assert!(body.get("reasoning").is_none());
}

#[test]
fn volcengine_bool_true_sends_enabled_thinking() {
    let config = ReasoningConfig::Bool(true);
    let mut body = base_body();

    VolcengineDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"enabled"}))
    );
    assert!(body.get("reasoning").is_none());
}

#[test]
fn volcengine_object_reasoning_sends_reasoning_only() {
    let config = ReasoningConfig::Object(serde_json::json!({"effort":"medium"}));
    let mut body = base_body();

    VolcengineDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(
        body.get("reasoning"),
        Some(&serde_json::json!({"effort":"medium"}))
    );
    assert!(body.get("thinking").is_none());
}

#[test]
fn minimax_bool_true_sends_adaptive_thinking_and_reasoning_split() {
    let config = ReasoningConfig::Bool(true);
    let mut body = base_body();

    MinimaxDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"adaptive"}))
    );
    assert_eq!(body.get("reasoning_split"), Some(&serde_json::json!(true)));
    assert!(body.get("reasoning").is_none());
}

#[test]
fn minimax_bool_false_sends_disabled_thinking_and_keeps_reasoning_split() {
    let config = ReasoningConfig::Bool(false);
    let mut body = base_body();

    MinimaxDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"disabled"}))
    );
    assert_eq!(body.get("reasoning_split"), Some(&serde_json::json!(true)));
}

#[test]
fn minimax_object_type_wins_over_reasoning_enabled() {
    let config = ReasoningConfig::Object(serde_json::json!({"type":"disabled"}));
    let mut body = base_body();

    MinimaxDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"disabled"}))
    );
    assert_eq!(body.get("reasoning_split"), Some(&serde_json::json!(true)));
}

#[test]
fn minimax_thinking_budget_uses_adaptive_thinking_without_budget_field() {
    let config = ReasoningConfig::ThinkingBudget(4096);
    let mut body = base_body();

    MinimaxDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"adaptive"}))
    );
    assert_eq!(body.get("reasoning_split"), Some(&serde_json::json!(true)));
    assert!(body.get("thinking_max_tokens").is_none());
}

// ===== #1393 — ThinkingBudget 不再生成 effort =====

#[test]
fn thinking_budget_for_scope_preserves_budget_for_openai() {
    // for_scope 在 budget 路径上必须保持 ThinkingBudget，不再改写为 Object(effort)
    let config = ReasoningConfig::ThinkingBudget(4096);
    let scoped = config.for_scope(ReasoningLevel::High, &OpenAiDriver);
    assert_eq!(scoped, ReasoningConfig::ThinkingBudget(4096));
}

#[test]
fn thinking_budget_for_scope_preserves_budget_for_litellm() {
    let config = ReasoningConfig::ThinkingBudget(40_000);
    let scoped = config.for_scope(ReasoningLevel::High, &LiteLlmDriver);
    assert_eq!(scoped, ReasoningConfig::ThinkingBudget(40_000));
}

#[test]
fn thinking_budget_for_scope_preserves_budget_for_volcengine() {
    // 即使对 Volcengine 这种 Object 改写 thinking 的 driver，
    // for_scope 也必须保留 ThinkingBudget，由 driver 自行决定是否启动 toggle。
    let config = ReasoningConfig::ThinkingBudget(8192);
    let scoped = config.for_scope(ReasoningLevel::Medium, &VolcengineDriver);
    assert_eq!(scoped, ReasoningConfig::ThinkingBudget(8192));
}

#[test]
fn thinking_budget_for_scope_off_returns_bool_false() {
    // 0 token / Off 语义保持不变
    let config = ReasoningConfig::ThinkingBudget(0);
    let scoped = config.for_scope(ReasoningLevel::Off, &OpenAiDriver);
    assert_eq!(scoped, ReasoningConfig::Bool(false));
}

#[test]
fn openai_provider_thinking_budget_does_not_generate_reasoning_or_effort() {
    let provider = openai_provider_with_reasoning_config(
        ProviderDriverKind::OpenAI,
        "openai",
        Some(ReasoningConfig::ThinkingBudget(4096)),
    );
    let scope = scope_with_effective_reasoning(ReasoningLevel::High);
    let mut body = base_body();

    provider.apply_reasoning_fields(&mut body, &scope);

    assert!(
        body.get("reasoning").is_none(),
        "OpenAI provider must not emit `reasoning` from a ThinkingBudget scope, got {body}"
    );
    assert!(
        body.get("reasoning_effort").is_none(),
        "OpenAI provider must not emit `reasoning_effort` from a ThinkingBudget scope, got {body}"
    );
}

#[test]
fn litellm_provider_thinking_budget_does_not_generate_reasoning_effort() {
    let provider = openai_provider_with_reasoning_config(
        ProviderDriverKind::LiteLLM,
        "litellm",
        Some(ReasoningConfig::ThinkingBudget(40_000)),
    );
    let scope = scope_with_effective_reasoning(ReasoningLevel::High);
    let mut body = base_body();

    provider.apply_reasoning_fields(&mut body, &scope);

    assert!(body.get("reasoning_effort").is_none());
    assert!(body.get("reasoning").is_none());
}

#[test]
fn volcengine_provider_thinking_budget_sends_enabled_thinking() {
    // 锁定 toggle 行为：for_scope 不再改写 budget 后，Volcengine 应仅发出 `thinking`。
    let provider = openai_provider_with_reasoning_config(
        ProviderDriverKind::Volcengine,
        "volcengine",
        Some(ReasoningConfig::ThinkingBudget(40_000)),
    );
    let scope = scope_with_effective_reasoning(ReasoningLevel::Medium);
    let mut body = base_body();

    provider.apply_reasoning_fields(&mut body, &scope);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"enabled"}))
    );
    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn minimax_provider_thinking_budget_keeps_adaptive_thinking_no_effort() {
    let provider = openai_provider_with_reasoning_config(
        ProviderDriverKind::Minimax,
        "minimax",
        Some(ReasoningConfig::ThinkingBudget(4096)),
    );
    let scope = scope_with_effective_reasoning(ReasoningLevel::Medium);
    let mut body = base_body();

    provider.apply_reasoning_fields(&mut body, &scope);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"adaptive"}))
    );
    assert_eq!(body.get("reasoning_split"), Some(&serde_json::json!(true)));
    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn mimo_provider_thinking_budget_keeps_enabled_thinking_no_effort() {
    let provider = openai_provider_with_reasoning_config(
        ProviderDriverKind::Mimo,
        "mimo",
        Some(ReasoningConfig::ThinkingBudget(8192)),
    );
    let scope = scope_with_effective_reasoning(ReasoningLevel::Medium);
    let mut body = base_body();

    provider.apply_reasoning_fields(&mut body, &scope);

    assert_eq!(
        body.get("thinking"),
        Some(&serde_json::json!({"type":"enabled"}))
    );
    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn agnes_provider_thinking_budget_keeps_enable_thinking_true_no_effort() {
    let provider = openai_provider_with_reasoning_config(
        ProviderDriverKind::Agnes,
        "agnes",
        Some(ReasoningConfig::ThinkingBudget(2048)),
    );
    let scope = scope_with_effective_reasoning(ReasoningLevel::Medium);
    let mut body = base_body();

    provider.apply_reasoning_fields(&mut body, &scope);

    assert_eq!(
        body.get("chat_template_kwargs"),
        Some(&serde_json::json!({"enable_thinking": true}))
    );
    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
}
