use crate::config_loader::load_config;
use aemeath_core::config::{models::ResolvedModel, Config, ModelEntryConfig};

/// 处理 `aemeath models` 子命令
fn format_token_limit_k(tokens: u32) -> String {
    if tokens > 0 {
        format!("{}k", tokens / 1000)
    } else {
        "-".to_string()
    }
}

fn model_row_display(
    provider: &str,
    model: &ModelEntryConfig,
) -> (String, String, String, String, String) {
    let name = if model.name.is_empty() {
        "-".to_string()
    } else {
        model.name.clone()
    };
    (
        provider.to_string(),
        model.id.clone(),
        name,
        format_token_limit_k(model.context_window as u32),
        format_token_limit_k(model.max_tokens),
    )
}

pub(crate) fn run_models_command(json: bool) {
    let config_file = load_config();
    match config_file {
        Some(cfg) => {
            let models = cfg.models.list_models();
            if models.is_empty() {
                eprintln!("No models configured. Add models to ~/.aemeath/config.json");
                std::process::exit(1);
            }
            if json {
                let output: Vec<serde_json::Value> = models
                    .iter()
                    .map(|(provider, m)| {
                        serde_json::json!({
                            "provider": provider,
                            "id": m.id,
                            "name": m.name,
                            "context_window": m.context_window,
                            "max_tokens": m.max_tokens,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                // 表格输出 — 自适应列宽
                let header = ("PROVIDER", "ID", "NAME", "CTX", "MAX");
                let rows: Vec<(String, String, String, String, String)> = models
                    .iter()
                    .map(|(provider, m)| model_row_display(provider, m))
                    .collect();

                let w0 = rows
                    .iter()
                    .map(|r| r.0.len())
                    .chain(std::iter::once(header.0.len()))
                    .max()
                    .unwrap_or(0);
                let w1 = rows
                    .iter()
                    .map(|r| r.1.len())
                    .chain(std::iter::once(header.1.len()))
                    .max()
                    .unwrap_or(0);
                let w2 = rows
                    .iter()
                    .map(|r| r.2.len())
                    .chain(std::iter::once(header.2.len()))
                    .max()
                    .unwrap_or(0);

                println!(
                    "{:<w$}  {:<w2$}  {:<w3$}  {:<w4$}  {}",
                    header.0,
                    header.1,
                    header.2,
                    header.3,
                    header.4,
                    w = w0,
                    w2 = w1,
                    w3 = w2,
                    w4 = header.3.len()
                );
                for (provider, id, name, ctx, max) in &rows {
                    println!(
                        "{:<w$}  {:<w2$}  {:<w3$}  {:<w4$}  {}",
                        provider,
                        id,
                        name,
                        ctx,
                        max,
                        w = w0,
                        w2 = w1,
                        w3 = w2,
                        w4 = header.3.len()
                    );
                }
            }
        }
        None => {
            eprintln!("No config file found. Create ~/.aemeath/config.json to configure models.");
            std::process::exit(1);
        }
    }
}

pub(crate) fn select_model_for_run(
    requested_model: Option<&str>,
    config_file: Option<&Config>,
) -> Result<ResolvedModel, String> {
    let cfg = config_file.ok_or_else(|| {
        "未指定模型。请使用 --model <来源>/<模型>，或在 ~/.aemeath/config.json 配置 models.default".to_string()
    })?;

    if let Some(selection) = requested_model.filter(|s| !s.trim().is_empty()) {
        cfg.models
            .resolve_model_selection(selection)
            .map_err(|e| e.to_string())
    } else {
        cfg.models
            .resolve_default_model()
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use aemeath_core::config::{Config, ModelEntryConfig, ModelsConfig, ProviderModelsConfig};
    use std::collections::HashMap;

    fn test_config_for_model_selection() -> Config {
        let mut providers = HashMap::new();
        providers.insert(
            "Zhipu".to_string(),
            ProviderModelsConfig {
                api: "zhipu".to_string(),
                api_key: "zhipu-key".to_string(),
                base_url: "https://zhipu.example.com".to_string(),
                models: vec![ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    max_tokens: 128000,
                    ..Default::default()
                }],
            },
        );
        providers.insert(
            "LiteLLM".to_string(),
            ProviderModelsConfig {
                api: "litellm".to_string(),
                api_key: "litellm-key".to_string(),
                base_url: "https://litellm.example.com".to_string(),
                models: vec![ModelEntryConfig {
                    id: "anthropic/claude-opus-4-7".to_string(),
                    max_tokens: 16000,
                    ..Default::default()
                }],
            },
        );
        Config {
            models: ModelsConfig {
                default: "Zhipu/glm-5.1".to_string(),
                providers,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_model_row_display_includes_max_tokens_as_k() {
        let model = ModelEntryConfig {
            id: "deepseek-v4-pro".to_string(),
            name: "DeepSeek V4 Pro".to_string(),
            context_window: 200_000,
            max_tokens: 8192,
            ..Default::default()
        };

        let row = super::model_row_display("DeepSeek", &model);
        assert_eq!(row.0, "DeepSeek");
        assert_eq!(row.1, "deepseek-v4-pro");
        assert_eq!(row.2, "DeepSeek V4 Pro");
        assert_eq!(row.3, "200k");
        assert_eq!(row.4, "8k");
    }

    #[test]
    fn test_model_row_display_zero_max_tokens_as_dash() {
        let model = ModelEntryConfig {
            id: "local".to_string(),
            context_window: 0,
            max_tokens: 0,
            ..Default::default()
        };

        let row = super::model_row_display("Ollama", &model);
        assert_eq!(row.2, "-");
        assert_eq!(row.3, "-");
        assert_eq!(row.4, "-");
    }

    #[test]
    fn test_select_model_prefers_cli_model() {
        let cfg = test_config_for_model_selection();
        let selected =
            super::select_model_for_run(Some("LiteLLM/anthropic/claude-opus-4-7"), Some(&cfg))
                .unwrap();
        assert_eq!(selected.source_key, "LiteLLM");
        assert_eq!(selected.model.id, "anthropic/claude-opus-4-7");
    }

    #[test]
    fn test_select_model_uses_config_default() {
        let cfg = test_config_for_model_selection();
        let selected = super::select_model_for_run(None, Some(&cfg)).unwrap();
        assert_eq!(selected.source_key, "Zhipu");
        assert_eq!(selected.model.id, "glm-5.1");
    }

    #[test]
    fn test_select_model_without_config_errors() {
        let err = super::select_model_for_run(None, None).unwrap_err();
        assert!(err.contains("未指定模型"));
    }
}
