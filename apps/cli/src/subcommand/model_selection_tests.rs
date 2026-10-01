fn model_summary(
    provider: &str,
    id: &str,
    name: &str,
    context_window: usize,
    max_tokens: u32,
) -> sdk::ModelSummary {
    sdk::ModelSummary {
        provider: provider.to_string(),
        id: id.to_string(),
        name: name.to_string(),
        context_window,
        max_tokens,
    }
}

#[test]
fn test_model_row_display_includes_max_tokens_as_k() {
    let model = model_summary(
        "DeepSeek",
        "deepseek-v4-pro",
        "DeepSeek V4 Pro",
        200_000,
        8192,
    );

    let row = super::model_row_display(&model);
    assert_eq!(row.0, "DeepSeek");
    assert_eq!(row.1, "deepseek-v4-pro");
    assert_eq!(row.2, "DeepSeek V4 Pro");
    assert_eq!(row.3, "200k");
    assert_eq!(row.4, "8k");
}

#[test]
fn test_model_row_display_zero_max_tokens_as_dash() {
    let model = model_summary("Ollama", "local", "", 0, 0);

    let row = super::model_row_display(&model);
    assert_eq!(row.2, "-");
    assert_eq!(row.3, "-");
    assert_eq!(row.4, "-");
}
