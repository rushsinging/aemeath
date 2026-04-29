//! Model command — change or show the current model.
//!
//! Registered via `inventory::submit!` for compile-time collection.

use crate::command::{Command, CommandAction, CommandCategory, CommandContext, CommandResult, CommandDescriptor};

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
            "/model <provider/model_id> - Switch to a different model".to_string(),
        ])
    })
}

fn model_execute(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let arg = args.trim();
    if arg.is_empty() {
        return CommandResult::Success(format!("Current model: {}", ctx.current_model));
    }
    if arg == "list" {
        let models = ctx.models_config.list_models();
        if models.is_empty() {
            return CommandResult::Success(
                "No models configured. Add models to ~/.aemeath/config.json under \"models.providers\"".to_string()
            );
        }
        let mut output = String::from("Available models:\n");
        let mut current_provider = String::new();
        for (provider_name, model) in &models {
            if *provider_name != current_provider {
                output.push_str(&format!("\n  [{}]\n", provider_name));
                current_provider = provider_name.clone();
            }
            let display_name = if model.name.is_empty() { &model.id } else { &model.name };
            let marker = if format!("{}/{}", provider_name, display_name) == ctx.current_model { " ←" } else { "" };
            output.push_str(&format!(
                "    {}/{} ctx:{}k max:{}k{}\n",
                provider_name, display_name,
                model.context_window / 1000, model.max_tokens / 1000, marker,
            ));
        }
        return CommandResult::Success(output);
    }
    match ctx.models_config.find_model(arg) {
        Some((_provider_name, provider_config, model)) => {
            CommandResult::Action(CommandAction::SwitchModel {
                provider_name: _provider_name,
                model_id: model.id.clone(),
                model_name: model.name.clone(),
                base_url: provider_config.base_url.clone(),
                api_key: provider_config.api_key.clone(),
                api_type: provider_config.api.clone(),
                max_tokens: model.max_tokens,
                context_window: model.context_window,
                reasoning: model.reasoning,
            })
        }
        None => {
            CommandResult::Error(format!(
                "Model '{}' not found. Use /model list to see available models.\nFormat: /model <provider>/<model_id>",
                arg
            ))
        }
    }
}
