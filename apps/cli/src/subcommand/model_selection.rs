use std::sync::Arc;

/// 处理 `aemeath models` 子命令
fn format_token_limit_k(tokens: u32) -> String {
    if tokens > 0 {
        format!("{}k", tokens / 1000)
    } else {
        "-".to_string()
    }
}

fn model_row_display(model: &sdk::ModelSummary) -> (String, String, String, String, String) {
    let name = if model.name.is_empty() {
        "-".to_string()
    } else {
        model.name.clone()
    };
    (
        model.provider.clone(),
        model.id.clone(),
        name,
        format_token_limit_k(model.context_window as u32),
        format_token_limit_k(model.max_tokens),
    )
}

pub(crate) async fn run_models_command(client: Arc<dyn sdk::AgentClient>, json: bool) {
    let models = client.list_models().await.unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });
    if models.is_empty() {
        eprintln!(
            "No models configured. Add models to ~/.agents/aemeath.json or .agents/aemeath.json"
        );
        std::process::exit(1);
    }
    if json {
        let output: Vec<serde_json::Value> = models
            .iter()
            .map(|m| {
                serde_json::json!({
                    "provider": m.provider,
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
        let rows: Vec<(String, String, String, String, String)> =
            models.iter().map(model_row_display).collect();

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

#[cfg(test)]
mod tests {
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
}
