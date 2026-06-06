//! Model command — change or show the current model.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::core::command::{
    Command, CommandAction, CommandCategory, CommandContext, CommandDescriptor, CommandResult,
};
use share::config::ModelsConfig;

inventory::submit! {
    CommandDescriptor::new(|| {
        Command::new(
            "model".to_string(),
            "Change or show the current model".to_string(),
            CommandCategory::Config,
            model_execute,
        )
        .with_usage(vec![
            "/model - Show current model".to_string(),
            "/model list - List available models from config".to_string(),
            "/model <source/model_id> - Switch to a different model".to_string(),
        ])
    })
}

fn model_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim();
    if arg.is_empty() {
        return CommandResult::Success(format!("Current model: {}", ctx.current_model));
    }
    if arg == "list" {
        return CommandResult::Success(format_model_list(&ctx.models_config, &ctx.current_model));
    }

    let selection = if arg.contains('/') {
        arg.to_string()
    } else if let Some(selection) = find_selection_by_fuzzy_model(&ctx.models_config, arg) {
        selection
    } else {
        return CommandResult::Error(format!(
            "Model '{}' not found. Use /model list to see available models.",
            arg
        ));
    };

    match ctx.models_config.resolve_model_selection(&selection) {
        Ok(resolved) => CommandResult::Action(CommandAction::SwitchModel {
            provider_name: resolved.source_key,
            model_id: resolved.model.id.clone(),
            model_name: resolved.model.name.clone(),
            base_url: resolved.source_config.base_url.clone(),
            api_key: resolved.source_config.api_key.clone(),
            driver: resolved.driver.as_str().to_string(),
            max_tokens: resolved.model.max_tokens,
            context_window: resolved.model.context_window,
            reasoning: resolved.model.reasoning,
        }),
        Err(e) => CommandResult::Error(format!(
            "Model '{}' not found. Use /model list to see available models.\n{}",
            arg, e
        )),
    }
}

fn format_model_list(models_config: &ModelsConfig, current_model: &str) -> String {
    let models = models_config.list_models();
    if models.is_empty() {
        return "No models configured. Add models to ~/.aemeath/config.json under \"models.providers\""
            .to_string();
    }

    let mut output = String::from("Available models:\n");
    let mut current_provider = String::new();
    for (provider_name, model) in &models {
        if *provider_name != current_provider {
            output.push_str(&format!("\n  [{}]\n", provider_name));
            current_provider = provider_name.clone();
        }
        let key = format!("{}/{}", provider_name, model.id);
        let marker = if key == current_model { " ←" } else { "" };
        output.push_str(&format!(
            "    {} ctx:{}k max:{}k{}\n",
            key,
            model.context_window / 1000,
            model.max_tokens / 1000,
            marker,
        ));
    }
    output
}

fn normalize_model_query(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn find_selection_by_fuzzy_model(models_config: &ModelsConfig, query: &str) -> Option<String> {
    let normalized_query = normalize_model_query(query);
    models_config
        .list_models()
        .into_iter()
        .find(|(_, model)| {
            model.name == query
                || model.id == query
                || normalize_model_query(&model.name) == normalized_query
                || normalize_model_query(&model.id) == normalized_query
        })
        .map(|(source, model)| format!("{}/{}", source, model.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::cost::CostTracker;
    use crate::business::state::AppState;
    use crate::core::command::{CommandAction, CommandResult};
    use share::config::{Config, ModelEntryConfig, ProviderModelsConfig};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn model_config_for_tests() -> ModelsConfig {
        let mut providers = HashMap::new();
        providers.insert(
            "Zhipu".to_string(),
            ProviderModelsConfig {
                driver: "zhipu".to_string(),
                api_key: "zhipu-key".to_string(),
                base_url: "https://zhipu.example.com".to_string(),
                models: vec![ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    name: "glm-5.1".to_string(),
                    max_tokens: 128000,
                    context_window: 128000,
                    reasoning: Some(true),
                    ..Default::default()
                }],
            },
        );
        providers.insert(
            "LiteLLM".to_string(),
            ProviderModelsConfig {
                driver: "litellm".to_string(),
                api_key: "litellm-key".to_string(),
                base_url: "https://litellm.example.com".to_string(),
                models: vec![ModelEntryConfig {
                    id: "anthropic/claude-opus-4-7".to_string(),
                    name: "claude-opus-4-7".to_string(),
                    max_tokens: 16000,
                    context_window: 200000,
                    reasoning: Some(false),
                    ..Default::default()
                }],
            },
        );
        ModelsConfig {
            providers,
            default: "Zhipu/glm-5.1".to_string(),
            ..Default::default()
        }
    }

    fn command_context() -> CommandContext {
        let models_config = model_config_for_tests();
        CommandContext {
            state: Arc::new(AppState::new()),
            config: Config::default(),
            cwd: ".".to_string(),
            session_id: "test".to_string(),
            verbose: false,
            cost_tracker: CostTracker::new(),
            models_config,
            current_model: "Zhipu/glm-5.1".to_string(),
            task_store: None,
        }
    }

    #[test]
    fn test_model_list_displays_source_model_keys() {
        let mut ctx = command_context();
        let result = model_execute("list", &mut ctx);
        let CommandResult::Success(output) = result else {
            panic!("expected success");
        };
        assert!(output.contains("Zhipu/glm-5.1"));
        assert!(output.contains("LiteLLM/anthropic/claude-opus-4-7"));
    }

    #[test]
    fn test_model_switch_litellm_slash_id_resolves_fields() {
        let mut ctx = command_context();
        let result = model_execute("LiteLLM/anthropic/claude-opus-4-7", &mut ctx);
        let CommandResult::Action(CommandAction::SwitchModel {
            provider_name,
            model_id,
            driver,
            reasoning,
            ..
        }) = result
        else {
            panic!("expected switch action");
        };
        assert_eq!(provider_name, "LiteLLM");
        assert_eq!(model_id, "anthropic/claude-opus-4-7");
        assert_eq!(driver, "litellm");
        assert_eq!(reasoning, Some(false));
    }

    #[test]
    fn test_model_switch_fuzzy_model_name_resolves_to_source_selection() {
        let mut ctx = command_context();
        let result = model_execute("glm-5.1", &mut ctx);
        let CommandResult::Action(CommandAction::SwitchModel {
            provider_name,
            model_id,
            driver,
            ..
        }) = result
        else {
            panic!("expected switch action");
        };
        assert_eq!(provider_name, "Zhipu");
        assert_eq!(model_id, "glm-5.1");
        assert_eq!(driver, "zhipu");
    }
}
