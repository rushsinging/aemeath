#[test]
fn custom_user_agent_is_sent_in_openai_headers() {
    let config = OpenAIProviderConfig::from_driver(crate::ProviderDriverKind::OpenAI, "openai");
    let provider = OpenAICompatibleProvider::new_with_user_agent(
        config,
        "test-key".to_string(),
        None,
        Some("test-model".to_string()),
        8192,
        false,
        None,
        30,
        "aemeath-test/1.0".to_string(),
    );

    assert_eq!(
        provider.build_headers().unwrap().get(reqwest::header::USER_AGENT).unwrap(),
        "aemeath-test/1.0"
    );
}

#[test]
fn minimax_provider_uses_max_completion_tokens_field() {
    let config =
        OpenAIProviderConfig::from_driver(crate::ProviderDriverKind::Minimax, "minimax");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        None,
        Some("MiniMax-M3".to_string()),
        32000,
        true,
        None,
        30,
    );

    let scope = crate::domain::invoke::InvocationScope::new(
        "MiniMax-M3",
        8192,
        crate::ports::ReasoningLevel::High,
        crate::ports::ReasoningLevel::High,
    )
    .unwrap();
    let body = provider.base_request_body(&scope, Vec::new(), false);
    assert_eq!(body.get("max_completion_tokens"), Some(&json!(8192)));
    assert!(body.get("max_tokens").is_none());
}

#[test]
fn minimax_provider_keeps_v1_base_url_suffix() {
    let config =
        OpenAIProviderConfig::from_driver(crate::ProviderDriverKind::Minimax, "minimax");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        Some("https://api.minimaxi.com/v1".to_string()),
        Some("MiniMax-M3".to_string()),
        32000,
        true,
        None,
        30,
    );

    assert_eq!(
        provider.chat_url(),
        "https://api.minimaxi.com/v1/chat/completions"
    );
}

#[test]
fn mimo_provider_keeps_v1_base_url_suffix() {
    let config = OpenAIProviderConfig::from_driver(crate::ProviderDriverKind::Mimo, "mimo");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        Some("https://token-plan-cn.xiaomimimo.com/v1".to_string()),
        Some("mimo-v2.5-pro".to_string()),
        8192,
        true,
        None,
        30,
    );

    assert_eq!(
        provider.chat_url(),
        "https://token-plan-cn.xiaomimimo.com/v1/chat/completions"
    );
}

#[test]
fn test_mimo_driver_uses_max_completion_tokens() {
    assert_eq!(MimoDriver.max_tokens_field(), "max_completion_tokens");
}

#[test]
fn test_mimo_driver_enables_thinking_by_default() {
    let mut body = serde_json::json!({});

    MimoDriver.apply_reasoning_fields(&mut body, None, false);

    assert_eq!(body["thinking"], serde_json::json!({ "type": "enabled" }));
}

#[test]
fn test_mimo_driver_respects_bool_reasoning_override() {
    let mut body = serde_json::json!({});

    MimoDriver.apply_reasoning_fields(&mut body, Some(&ReasoningConfig::Bool(false)), true);

    assert_eq!(body["thinking"], serde_json::json!({ "type": "disabled" }));
}

#[test]
fn test_mimo_config_uses_chat_completions_suffix() {
    use crate::ProviderDriverKind;
    let config = OpenAIProviderConfig::from_driver(ProviderDriverKind::Mimo, "mimo");

    assert_eq!(config.chat_api_suffix, "/chat/completions");
    assert_eq!(config.driver, ProviderDriverKind::Mimo);
}

// === Zhipu effort ===

#[test]
fn zhipu_object_effort_sends_thinking_and_reasoning_effort() {
    let config = ReasoningConfig::Object(json!({"effort": "high"}));
    let mut body = base_body();

    ZhipuDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("thinking"), Some(&json!({"type": "enabled"})));
    assert_eq!(body.get("reasoning_effort"), Some(&json!("high")));
}

#[test]
fn zhipu_disabled_omits_reasoning_effort() {
    let config = ReasoningConfig::Bool(false);
    let mut body = base_body();

    ZhipuDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(body.get("thinking"), Some(&json!({"type": "disabled"})));
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn zhipu_bool_true_without_effort_omits_reasoning_effort() {
    let config = ReasoningConfig::Bool(true);
    let mut body = base_body();

    ZhipuDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("thinking"), Some(&json!({"type": "enabled"})));
    // Bool(true) 没有 effort 信息，不发 reasoning_effort
    assert!(body.get("reasoning_effort").is_none());
}

// === DeepSeek ===

#[test]
fn deepseek_object_effort_sends_thinking_and_reasoning_effort() {
    let config = ReasoningConfig::Object(json!({"effort": "max"}));
    let mut body = base_body();

    DeepSeekDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(body.get("thinking"), Some(&json!({"type": "enabled"})));
    assert_eq!(body.get("reasoning_effort"), Some(&json!("max")));
}

#[test]
fn deepseek_disabled_sends_disabled_thinking() {
    let config = ReasoningConfig::Bool(false);
    let mut body = base_body();

    DeepSeekDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(body.get("thinking"), Some(&json!({"type": "disabled"})));
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn deepseek_enabled_without_effort_sends_no_reasoning_effort() {
    let mut body = base_body();

    DeepSeekDriver.apply_reasoning_fields(&mut body, None, true);

    assert_eq!(body.get("thinking"), Some(&json!({"type": "enabled"})));
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn test_deepseek_config_uses_chat_completions_suffix() {
    use crate::ProviderDriverKind;
    let config = OpenAIProviderConfig::from_driver(ProviderDriverKind::DeepSeek, "deepseek");

    assert_eq!(config.chat_api_suffix, "/chat/completions");
    assert_eq!(config.driver, ProviderDriverKind::DeepSeek);
}

// === Agnes ===

#[test]
fn agnes_bool_true_sends_enable_thinking_true() {
    let config = ReasoningConfig::Bool(true);
    let mut body = base_body();

    AgnesDriver.apply_reasoning_fields(&mut body, Some(&config), true);

    assert_eq!(
        body.get("chat_template_kwargs"),
        Some(&json!({"enable_thinking": true}))
    );
    assert!(body.get("thinking").is_none());
    assert!(body.get("reasoning").is_none());
}

#[test]
fn agnes_bool_false_sends_enable_thinking_false() {
    let config = ReasoningConfig::Bool(false);
    let mut body = base_body();

    AgnesDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(
        body.get("chat_template_kwargs"),
        Some(&json!({"enable_thinking": false}))
    );
}

#[test]
fn agnes_none_uses_reasoning_enabled_flag() {
    let mut body = base_body();

    AgnesDriver.apply_reasoning_fields(&mut body, None, true);

    assert_eq!(
        body.get("chat_template_kwargs"),
        Some(&json!({"enable_thinking": true}))
    );

    let mut body2 = base_body();
    AgnesDriver.apply_reasoning_fields(&mut body2, None, false);
    assert_eq!(
        body2.get("chat_template_kwargs"),
        Some(&json!({"enable_thinking": false}))
    );
}

#[test]
fn agnes_object_config_enables_thinking() {
    let config = ReasoningConfig::Object(json!({"effort": "high"}));
    let mut body = base_body();

    AgnesDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    // Object 配置视为启用 thinking（Agnes 不支持 effort 分级，忽略 effort 值）
    assert_eq!(
        body.get("chat_template_kwargs"),
        Some(&json!({"enable_thinking": true}))
    );
}

#[test]
fn agnes_thinking_budget_enables_thinking() {
    let config = ReasoningConfig::ThinkingBudget(4096);
    let mut body = base_body();

    AgnesDriver.apply_reasoning_fields(&mut body, Some(&config), false);

    assert_eq!(
        body.get("chat_template_kwargs"),
        Some(&json!({"enable_thinking": true}))
    );
}

#[test]
fn test_agnes_config_uses_chat_completions_suffix() {
    use crate::ProviderDriverKind;
    let config = OpenAIProviderConfig::from_driver(ProviderDriverKind::Agnes, "agnes");

    assert_eq!(config.chat_api_suffix, "/chat/completions");
    assert_eq!(config.driver, ProviderDriverKind::Agnes);
}

#[test]
fn agnes_provider_keeps_v1_base_url_suffix() {
    let config = OpenAIProviderConfig::from_driver(crate::ProviderDriverKind::Agnes, "agnes");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        Some("https://apihub.agnes-ai.com/v1".to_string()),
        Some("agnes-2.0-flash".to_string()),
        4096,
        true,
        None,
        30,
    );

    assert_eq!(
        provider.chat_url(),
        "https://apihub.agnes-ai.com/v1/chat/completions"
    );
}

#[test]
fn test_from_str_agnes() {
    use crate::ProviderDriverKind;
    assert_eq!(
        ProviderDriverKind::parse("agnes"),
        Some(ProviderDriverKind::Agnes)
    );
}

#[test]
fn test_as_str_agnes() {
    use crate::ProviderDriverKind;
    assert_eq!(ProviderDriverKind::Agnes.as_str(), "agnes");
}

#[test]
fn openai_provider_config_from_driver_sets_fields() {
    use crate::ProviderDriverKind;
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
fn openai_provider_uses_scope_max_tokens_in_request_body() {
    let config =
        OpenAIProviderConfig::from_driver(crate::ProviderDriverKind::OpenAI, "openai");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        None,
        Some("test-model".to_string()),
        32000,
        false,
        None,
        30,
    );

    // max_tokens is now immutable; the scope carries the per-request value
    let scope = crate::domain::invoke::InvocationScope::new(
        "test-model",
        8192,
        crate::ports::ReasoningLevel::Off,
        crate::ports::ReasoningLevel::Off,
    )
    .unwrap();
    let body = provider.base_request_body(&scope, Vec::new(), false);
    assert_eq!(body.get("max_tokens"), Some(&json!(8192)));
}

#[test]
fn invocation_scope_requires_non_zero_max_tokens() {
    let config =
        OpenAIProviderConfig::from_driver(crate::ProviderDriverKind::OpenAI, "openai");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        None,
        Some("test-model".to_string()),
        32000,
        false,
        None,
        30,
    );

    // InvocationScope enforces max_tokens > 0 at construction time
    let scope = crate::domain::invoke::InvocationScope::new(
        "test-model",
        32000,
        crate::ports::ReasoningLevel::Off,
        crate::ports::ReasoningLevel::Off,
    )
    .unwrap();
    let body = provider.base_request_body(&scope, Vec::new(), false);
    assert_eq!(body.get("max_tokens"), Some(&json!(32000)));
}

#[test]
fn volcengine_provider_uses_max_output_tokens_field() {
    let config =
        OpenAIProviderConfig::from_driver(crate::ProviderDriverKind::Volcengine, "volcengine");
    let provider = OpenAICompatibleProvider::new(
        config,
        "test-key".to_string(),
        None,
        Some("test-model".to_string()),
        32000,
        false,
        None,
        30,
    );

    let scope = crate::domain::invoke::InvocationScope::new(
        "test-model",
        8192,
        crate::ports::ReasoningLevel::Off,
        crate::ports::ReasoningLevel::Off,
    )
    .unwrap();
    let body = provider.base_request_body(&scope, Vec::new(), false);
    assert_eq!(body.get("max_output_tokens"), Some(&json!(8192)));
    assert!(body.get("max_tokens").is_none());
}

#[test]
fn openai_streaming_http_client_uses_connect_timeout() {
    let debug = format!("{:?}", provider::build_streaming_http_client_builder(30));

    assert!(debug.contains("connect_timeout"), "{debug}");
}
