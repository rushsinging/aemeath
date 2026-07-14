use super::*;

fn budget(context_size: usize, max_output_tokens: usize) -> TokenBudgetConfig {
    TokenBudgetConfig::new(context_size, max_output_tokens)
}

#[test]
fn test_token_budget_config_uses_model_max_output_tokens() {
    let small_output = budget(200_000, 4_096);
    let large_output = budget(200_000, 16_000);

    assert_eq!(small_output.context_size(), 200_000);
    assert_eq!(small_output.max_output_tokens(), 4_096);
    assert_eq!(small_output.effective_context_window(), 195_904);
    assert_eq!(large_output.effective_context_window(), 184_000);
    assert!(small_output.autocompact_threshold() > large_output.autocompact_threshold());
}

#[test]
fn test_token_budget_config_caps_summary_reservation() {
    let capped = budget(200_000, 30_000);

    assert_eq!(capped.effective_context_window(), 180_000);
    assert_eq!(capped.autocompact_threshold(), 133_600);
}

#[test]
fn test_token_budget_config_saturates_for_small_context_window() {
    let config = budget(10_000, 20_000);

    assert_eq!(config.effective_context_window(), 0);
    assert_eq!(config.autocompact_threshold(), 0);
}

#[test]
fn test_needs_compaction_actual_uses_budget_config() {
    let small_output = budget(200_000, 4_096);
    let large_output = budget(200_000, 16_000);

    assert!(!needs_compaction_actual(140_000, 5_000, &small_output));
    assert!(needs_compaction_actual(140_000, 5_000, &large_output));
}

#[test]
fn test_needs_compaction_actual_counts_output_once() {
    let config = budget(1_048_576, 8_192);

    assert!(!needs_compaction_actual(50_000, 10_000, &config));
    assert!(needs_compaction_actual(800_000, 30_000, &config));
}

#[test]
fn test_needs_compaction_full_includes_tool_schemas() {
    let config = budget(50_000, 4_096);
    let threshold = config.autocompact_threshold();

    assert!(!needs_compaction_full(&[], "", threshold, &config));
    assert!(needs_compaction_full(&[], "", threshold + 1, &config));
}

#[test]
fn test_compaction_urgency_uses_effective_window() {
    let config = budget(1_048_576, 8_192);

    assert_eq!(compaction_urgency(700_000, &config), 0);
    assert_eq!(compaction_urgency(730_000, &config), 1);
    assert_eq!(compaction_urgency(840_000, &config), 2);
    assert_eq!(compaction_urgency(940_000, &config), 3);
}

#[test]
fn test_estimate_ascii() {
    let tokens = estimate_tokens("hello world");
    assert!((3..=5).contains(&tokens));
}

#[test]
fn test_estimate_cjk() {
    let tokens = estimate_tokens("你好世界");
    assert!(tokens >= 8);
}

#[test]
fn test_format_tokens() {
    assert_eq!(format_tokens(500), "500");
    assert_eq!(format_tokens(1500), "1.5k");
    assert_eq!(format_tokens(15000), "15k");
    assert_eq!(format_tokens(1500000), "1.5m");
}
