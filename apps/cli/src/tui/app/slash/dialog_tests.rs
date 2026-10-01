use super::build_model_dialog_options;

fn model(provider: &str, name: &str) -> sdk::ModelSummary {
    sdk::ModelSummary {
        provider: provider.to_string(),
        id: format!("{provider}-id"),
        name: name.to_string(),
        context_window: 200_000,
        max_tokens: 8_000,
    }
}

#[test]
fn test_build_model_dialog_options_empty_yields_no_options() {
    let (options, keys) = build_model_dialog_options(&[], "anthropic/claude");
    assert!(options.is_empty());
    assert!(keys.is_empty());
}

#[test]
fn test_build_model_dialog_options_multiple_with_marker() {
    let models = vec![model("anthropic", "claude"), model("openai", "gpt")];
    let (options, keys) = build_model_dialog_options(&models, "anthropic/claude");
    assert_eq!(keys, vec!["anthropic/claude", "openai/gpt"]);
    assert!(options[0].contains("anthropic/claude ctx:200k max:8k ←"));
    assert!(options[1].contains("openai/gpt ctx:200k max:8k"));
    assert!(!options[1].contains('←'));
}

#[test]
fn test_build_model_dialog_options_empty_name_falls_back_to_id() {
    let models = vec![model("ollama", "")];
    let (options, keys) = build_model_dialog_options(&models, "");
    assert_eq!(keys, vec!["ollama/ollama-id"]);
    assert!(options[0].contains("ollama/ollama-id"));
}
