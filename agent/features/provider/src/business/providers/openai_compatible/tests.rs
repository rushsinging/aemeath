use super::driver::{
    ChatApiDriver, LiteLlmDriver, MinimaxDriver, OpenAiDriver, VolcengineDriver, ZhipuDriver,
};
use super::*;
use crate::core::client::OpenAIProviderConfig;
use crate::core::provider::LlmProvider;
use serde_json::json;

fn base_body() -> serde_json::Value {
    json!({"model":"test-model","messages":[],"max_tokens":10,"stream":true})
}

fn assert_no_reasoning_fields(body: &serde_json::Value) {
    assert!(body.get("reasoning").is_none());
    assert!(body.get("thinking").is_none());
    assert!(body.get("enable_thinking").is_none());
}

#[test]
fn openai_object_reasoning_sends_reasoning_only() {
    let config = ReasoningConfig::Object(json!({"effort":"medium"}));
    let mut body = base_body();

    OpenAiDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("reasoning"), Some(&json!({"effort":"medium"})));
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

    assert_eq!(body.get("thinking"), Some(&json!({"type":"enabled"})));
    assert!(body.get("reasoning").is_none());
}

#[test]
fn zhipu_bool_false_sends_disabled_thinking() {
    let config = ReasoningConfig::Bool(false);
    let mut body = base_body();

    ZhipuDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(body.get("thinking"), Some(&json!({"type":"disabled"})));
    assert!(body.get("reasoning").is_none());
}

#[test]
fn litellm_object_reasoning_sends_reasoning_effort() {
    let config = ReasoningConfig::Object(json!({"effort":"high"}));
    let mut body = base_body();

    LiteLlmDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("reasoning_effort"), Some(&json!("high")));
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
fn openai_thinking_budget_maps_to_medium_reasoning_effort() {
    let config = ReasoningConfig::ThinkingBudget(4096);
    let mut body = base_body();

    OpenAiDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("reasoning"), Some(&json!({"effort":"medium"})));
}

#[test]
fn litellm_thinking_budget_maps_to_top_level_reasoning_effort() {
    let config = ReasoningConfig::ThinkingBudget(40000);
    let mut body = base_body();

    LiteLlmDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("reasoning_effort"), Some(&json!("xhigh")));
    assert!(body.get("reasoning").is_none());
}

#[test]
fn thinking_budget_effort_boundaries() {
    assert_eq!(effort_from_thinking_tokens(1024), "low");
    assert_eq!(effort_from_thinking_tokens(1025), "medium");
    assert_eq!(effort_from_thinking_tokens(8193), "high");
    assert_eq!(effort_from_thinking_tokens(32769), "xhigh");
}

#[test]
fn volcengine_thinking_budget_sends_enabled_thinking() {
    let config = ReasoningConfig::ThinkingBudget(40000);
    let mut body = base_body();

    VolcengineDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    // Volcengine 使用 thinking.type 字段，ThinkingBudget 表示启用。
    assert_eq!(body.get("thinking"), Some(&json!({"type":"enabled"})));
    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn volcengine_bool_false_sends_disabled_thinking() {
    let config = ReasoningConfig::Bool(false);
    let mut body = base_body();

    VolcengineDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(body.get("thinking"), Some(&json!({"type":"disabled"})));
    assert!(body.get("reasoning").is_none());
}

#[test]
fn volcengine_bool_true_sends_enabled_thinking() {
    let config = ReasoningConfig::Bool(true);
    let mut body = base_body();

    VolcengineDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("thinking"), Some(&json!({"type":"enabled"})));
    assert!(body.get("reasoning").is_none());
}

#[test]
fn volcengine_object_reasoning_sends_reasoning_only() {
    let config = ReasoningConfig::Object(json!({"effort":"medium"}));
    let mut body = base_body();

    VolcengineDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("reasoning"), Some(&json!({"effort":"medium"})));
    assert!(body.get("thinking").is_none());
}

#[test]
fn minimax_bool_true_sends_adaptive_thinking_and_reasoning_split() {
    let config = ReasoningConfig::Bool(true);
    let mut body = base_body();

    MinimaxDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("thinking"), Some(&json!({"type":"adaptive"})));
    assert_eq!(body.get("reasoning_split"), Some(&json!(true)));
    assert!(body.get("reasoning").is_none());
}

#[test]
fn minimax_bool_false_sends_disabled_thinking_and_keeps_reasoning_split() {
    let config = ReasoningConfig::Bool(false);
    let mut body = base_body();

    MinimaxDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("thinking"), Some(&json!({"type":"disabled"})));
    assert_eq!(body.get("reasoning_split"), Some(&json!(true)));
}

#[test]
fn minimax_object_type_wins_over_reasoning_enabled() {
    let config = ReasoningConfig::Object(json!({"type":"disabled"}));
    let mut body = base_body();

    MinimaxDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("thinking"), Some(&json!({"type":"disabled"})));
    assert_eq!(body.get("reasoning_split"), Some(&json!(true)));
}

#[test]
fn minimax_thinking_budget_uses_adaptive_thinking_without_budget_field() {
    let config = ReasoningConfig::ThinkingBudget(4096);
    let mut body = base_body();

    MinimaxDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(body.get("thinking"), Some(&json!({"type":"adaptive"})));
    assert_eq!(body.get("reasoning_split"), Some(&json!(true)));
    assert!(body.get("thinking_max_tokens").is_none());
}

#[test]
fn minimax_provider_uses_max_completion_tokens_field() {
    let config =
        OpenAIProviderConfig::from_driver(crate::api::ProviderDriverKind::Minimax, "minimax");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        None,
        Some("MiniMax-M3".to_string()),
        32000,
        true,
        None,
    );

    provider.set_max_tokens(8192);
    let body = provider.base_request_body(Vec::new(), false);
    assert_eq!(body.get("max_completion_tokens"), Some(&json!(8192)));
    assert!(body.get("max_tokens").is_none());
}

#[test]
fn minimax_provider_keeps_v1_base_url_suffix() {
    let config =
        OpenAIProviderConfig::from_driver(crate::api::ProviderDriverKind::Minimax, "minimax");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        Some("https://api.minimaxi.com/v1".to_string()),
        Some("MiniMax-M3".to_string()),
        32000,
        true,
        None,
    );

    assert_eq!(
        provider.chat_url(),
        "https://api.minimaxi.com/v1/chat/completions"
    );
}

#[test]
fn openai_provider_config_from_driver_sets_fields() {
    use crate::api::ProviderDriverKind;
    let openai = OpenAIProviderConfig::from_driver(ProviderDriverKind::OpenAI, "source-openai");
    assert_eq!(openai.source_key, "source-openai");
    assert_eq!(openai.driver, ProviderDriverKind::OpenAI);
    assert_eq!(openai.chat_api_suffix, "/v1/chat/completions");

    let zhipu = OpenAIProviderConfig::from_driver(ProviderDriverKind::Zhipu, "source-zhipu");
    assert_eq!(zhipu.source_key, "source-zhipu");
    assert_eq!(zhipu.driver, ProviderDriverKind::Zhipu);
    assert_eq!(zhipu.chat_api_suffix, "/chat/completions");

    let volcengine =
        OpenAIProviderConfig::from_driver(ProviderDriverKind::Volcengine, "source-volcengine");
    assert_eq!(volcengine.source_key, "source-volcengine");
    assert_eq!(volcengine.driver, ProviderDriverKind::Volcengine);
    assert_eq!(volcengine.chat_api_suffix, "/chat/completions");

    let minimax = OpenAIProviderConfig::from_driver(ProviderDriverKind::Minimax, "source-minimax");
    assert_eq!(minimax.source_key, "source-minimax");
    assert_eq!(minimax.driver, ProviderDriverKind::Minimax);
    assert_eq!(minimax.chat_api_suffix, "/chat/completions");
}

#[test]
fn openai_provider_set_max_tokens_updates_request_body() {
    let config =
        OpenAIProviderConfig::from_driver(crate::api::ProviderDriverKind::OpenAI, "openai");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        None,
        Some("test-model".to_string()),
        32000,
        false,
        None,
    );

    provider.set_max_tokens(8192);
    let body = provider.base_request_body(Vec::new(), false);
    assert_eq!(body.get("max_tokens"), Some(&json!(8192)));
}

#[test]
fn openai_provider_set_max_tokens_zero_is_ignored() {
    let config =
        OpenAIProviderConfig::from_driver(crate::api::ProviderDriverKind::OpenAI, "openai");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        None,
        Some("test-model".to_string()),
        32000,
        false,
        None,
    );

    provider.set_max_tokens(0);
    let body = provider.base_request_body(Vec::new(), false);
    assert_eq!(body.get("max_tokens"), Some(&json!(32000)));
}

#[test]
fn volcengine_provider_uses_max_output_tokens_field() {
    let config =
        OpenAIProviderConfig::from_driver(crate::api::ProviderDriverKind::Volcengine, "volcengine");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        None,
        Some("test-model".to_string()),
        32000,
        false,
        None,
    );

    provider.set_max_tokens(8192);
    let body = provider.base_request_body(Vec::new(), false);
    assert_eq!(body.get("max_output_tokens"), Some(&json!(8192)));
    assert!(body.get("max_tokens").is_none());
}

#[test]
fn openai_streaming_http_client_has_no_total_timeout() {
    let debug = format!("{:?}", provider::build_streaming_http_client_builder());

    assert!(!debug.contains("timeout:"), "{debug}");
}
