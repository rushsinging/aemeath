use crate::tui::effect::effect::Effect;
use crate::tui::render::dialog::Dialog;

impl super::super::App {
    /// 打开 /model 选择对话框：先用已回填的缓存呈现（立即打开 / 挂起等待 /
    /// 空态提示），并附带一次 `ListModels` 刷新请求（#740）。
    pub(super) fn open_model_selection_dialog(&mut self) -> Effect {
        self.present_model_selection_dialog();
        Effect::SendChatInputEvent {
            event: sdk::ChatInputEvent::ListModels,
        }
    }

    /// 用当前缓存呈现 /model 对话框（纯呈现，不发请求）：
    /// - 缓存未回填（`None`）→ 挂起等待 `ModelList` 事件回填后自动打开；
    /// - 已回填且为空 → 提示真实配置路径；
    /// - 已回填非空 → 打开选择对话框。
    pub(crate) fn present_model_selection_dialog(&mut self) {
        let current = self.session.current_model_display.clone();
        let cached = self.session.cached_models.clone().unwrap_or_default();
        let (options, keys) = build_model_dialog_options(&cached, &current);
        if options.is_empty() {
            if self.session.cached_models.is_none() {
                self.session.model_selection_pending = true;
                self.append_system_notice("Loading model list…");
            } else {
                self.append_system_notice(
                    "No models configured. Add models to ~/.agents/aemeath.json or .agents/aemeath.json",
                );
            }
            return;
        }
        self.layout
            .open_model_dialog(Dialog::select("Select Model", options), keys);
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
