use ::runtime::api::core::config::{ConfigManager, ModelEntryConfig};

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

pub(crate) async fn run_models_command(json: bool) {
    let cwd = std::env::current_dir().ok();
    let manager = ConfigManager::new(cwd.as_deref());
    let config_file = manager.load().await.ok();
    match config_file {
        Some(cfg) => {
            let models = cfg.models.list_models();
            if models.is_empty() {
                eprintln!(
                    "No models configured. Add models to ~/.agents/aemeath.json or .agents/aemeath.json"
                );
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
            eprintln!("No config file found. Create ~/.agents/aemeath.json to configure models.");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use ::runtime::api::core::config::ModelEntryConfig;

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
}
