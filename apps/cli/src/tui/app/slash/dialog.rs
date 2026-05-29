use crate::tui::render::dialog::Dialog;

impl super::super::App {
    pub(super) fn open_model_selection_dialog(&mut self) -> Option<String> {
        let models = if let Some(agent_client) = &self.agent_client {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(agent_client.list_models())
                    .unwrap_or_default()
            })
        } else {
            Vec::new()
        };
        if models.is_empty() {
            self.append_system_notice("No models configured. Add models to ~/.aemeath/config.json");
            return None;
        }
        let current = self.session.current_model_display.clone();
        let mut options = Vec::new();
        let mut keys = Vec::new();
        for model in &models {
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
        self.layout
            .open_model_dialog(Dialog::select("Select Model", options), keys);
        None
    }
}
