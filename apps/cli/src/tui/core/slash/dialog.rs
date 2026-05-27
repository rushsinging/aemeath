use crate::tui::display::dialog::Dialog;

impl super::super::App {
    pub(super) fn open_model_selection_dialog(&mut self) -> Option<String> {
        let models = self.cmd_exec.models_config.list_models();
        if models.is_empty() {
            self.output_area
                .push_system("No models configured. Add models to ~/.aemeath/config.json");
            return None;
        }
        let current = self.session.current_model_display.clone();
        let mut options = Vec::new();
        let mut keys = Vec::new();
        for (provider_name, model) in &models {
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
        self.layout.active_dialog = Some(Dialog::select("Select Model", options));
        self.layout.dialog_model_keys = keys;
        None
    }
}
