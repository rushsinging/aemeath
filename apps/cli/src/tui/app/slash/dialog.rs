use crate::tui::render::dialog::Dialog;

impl super::super::App {
    pub(super) fn open_model_selection_dialog(&mut self) -> Option<String> {
        // #567：模型列表走事件流（ListModels），缓存尚未接入。暂传空列表。
        let current = self.session.current_model_display.clone();
        let (options, keys) = build_model_dialog_options(&[], &current);
        if options.is_empty() {
            self.append_system_notice("No models configured. Add models to ~/.aemeath/config.json");
            return None;
        }
        self.layout
            .open_model_dialog(Dialog::select("Select Model", options), keys);
        None
    }
}

/// 由缓存的模型列表构建对话框选项与对应的选择 key（纯函数）。
fn build_model_dialog_options(
    models: &[sdk::ModelSummary],
    current: &str,
) -> (Vec<String>, Vec<String>) {
    let mut options = Vec::new();
    let mut keys = Vec::new();
    for model in models {
        let provider_name = &model.provider;
        let display_name = if model.name.is_empty() {
            &model.id
        } else {
            &model.name
        };
        let key = format!("{provider_name}/{display_name}");
        let marker = if key == current { " ←" } else { "" };
        options.push(format!(
            "{}/{} ctx:{}k max:{}k{}",
            provider_name,
            display_name,
            model.context_window / 1000,
            model.max_tokens / 1000,
            marker,
        ));
        keys.push(key);
    }
    (options, keys)
}

#[cfg(test)]
mod tests {
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
}
