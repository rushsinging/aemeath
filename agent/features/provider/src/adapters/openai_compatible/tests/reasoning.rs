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

