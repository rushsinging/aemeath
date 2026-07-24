// === clamp_effort ===

#[test]
fn test_clamp_effort_openai_wire_none_for_off() {
    let driver = OpenAiDriver;
    assert_eq!(driver.clamp_effort("off"), "none");
    assert_eq!(driver.clamp_effort("none"), "none");
}

#[test]
fn test_clamp_effort_openai_passthrough_all_levels() {
    let driver = OpenAiDriver;
    // OpenAI now supports all levels including Minimal and Max
    for level in &["minimal", "low", "medium", "high", "xhigh", "max"] {
        assert_eq!(driver.clamp_effort(level), *level);
    }
}

#[test]
fn test_clamp_effort_volcengine_downgrades_high_above() {
    let driver = VolcengineDriver;
    assert_eq!(driver.clamp_effort("low"), "low");
    assert_eq!(driver.clamp_effort("medium"), "medium");
    assert_eq!(driver.clamp_effort("high"), "medium");
    assert_eq!(driver.clamp_effort("xhigh"), "medium");
    assert_eq!(driver.clamp_effort("max"), "medium");
}

#[test]
fn test_clamp_effort_zhipu_passthrough_all_levels() {
    let driver = ZhipuDriver;
    for level in &["low", "medium", "high", "xhigh", "max"] {
        assert_eq!(driver.clamp_effort(level), *level);
    }
}

#[test]
fn test_clamp_effort_deepseek_passthrough_all_levels() {
    let driver = DeepSeekDriver;
    for level in &["low", "medium", "high", "xhigh", "max"] {
        assert_eq!(driver.clamp_effort(level), *level);
    }
}

#[test]
fn test_clamp_effort_litellm_passthrough_all_levels() {
    let driver = LiteLlmDriver;
    for level in &["low", "medium", "high", "xhigh", "max"] {
        assert_eq!(driver.clamp_effort(level), *level);
    }
}

#[test]
fn test_clamp_effort_minimax_derives_toggle_level_from_capability() {
    let driver = MinimaxDriver;
    assert_eq!(driver.clamp_effort("high"), "medium");
}

#[test]
fn test_clamp_effort_agnes_derives_toggle_level_from_capability() {
    let driver = AgnesDriver;
    assert_eq!(driver.clamp_effort("max"), "medium");
}

// === ReasoningConfig::clamped ===

#[test]
fn test_clamped_object_passthrough_max_for_openai() {
    // OpenAI now supports all levels up to Max — no downgrade needed.
    let config = ReasoningConfig::Object(json!({"effort": "max"}));
    let clamped = config.clamped(&OpenAiDriver);
    assert_eq!(clamped, config);
}

#[test]
fn test_clamped_object_unchanged_when_within_range() {
    let config = ReasoningConfig::Object(json!({"effort": "medium"}));
    let clamped = config.clamped(&OpenAiDriver);
    assert_eq!(clamped, config);
}

#[test]
fn test_clamped_object_downgrades_effort_for_volcengine() {
    let config = ReasoningConfig::Object(json!({"effort": "high"}));
    let clamped = config.clamped(&VolcengineDriver);
    assert_eq!(
        clamped,
        ReasoningConfig::Object(json!({"effort": "medium"}))
    );
}

#[test]
fn test_clamped_thinking_budget_remains_independent_from_effort() {
    let config = ReasoningConfig::ThinkingBudget(40000);
    let clamped = config.clamped(&OpenAiDriver);
    assert_eq!(clamped, config);
}

#[test]
fn test_clamped_bool_unchanged() {
    let config = ReasoningConfig::Bool(true);
    let clamped = config.clamped(&OpenAiDriver);
    assert_eq!(clamped, config);
}

#[test]
fn test_clamped_object_without_effort_unchanged() {
    let config = ReasoningConfig::Object(json!({"type": "disabled"}));
    let clamped = config.clamped(&OpenAiDriver);
    assert_eq!(clamped, config);
}
