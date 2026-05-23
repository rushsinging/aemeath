//! 模型建议生成

use super::types::{Suggestion, SuggestionType};

/// 根据 /model 命令的部分输入生成模型建议
pub fn generate_model_suggestions(partial: &str, models: &[(String, String)]) -> Vec<Suggestion> {
    if models.is_empty() {
        return Vec::new();
    }

    let partial_lower = partial.to_lowercase();

    models
        .iter()
        .filter(|(provider, model_id)| {
            let full = format!("{}/{}", provider, model_id);
            full.to_lowercase().starts_with(&partial_lower)
                || provider.to_lowercase().starts_with(&partial_lower)
        })
        .map(|(provider, model_id)| Suggestion {
            _id: format!("model-{}/{}", provider, model_id),
            display_text: format!("{}/{}", provider, model_id),
            _description: None,
            suggestion_type: SuggestionType::Model,
        })
        .collect()
}

/// 为 /model 命令生成子命令建议
pub fn generate_model_subcommand_suggestions(partial: &str) -> Vec<Suggestion> {
    let subcommands = vec![("list", "List available models from config")];

    let partial_lower = partial.to_lowercase();

    subcommands
        .iter()
        .filter(|(name, _desc)| name.to_lowercase().starts_with(&partial_lower))
        .map(|(name, desc)| Suggestion {
            _id: format!("model-subcmd-{}", name),
            display_text: format!("list"),
            _description: Some(desc.to_string()),
            suggestion_type: SuggestionType::Command,
        })
        .collect()
}
