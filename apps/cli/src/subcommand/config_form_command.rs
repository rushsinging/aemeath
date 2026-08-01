#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigFormEffect {
    SubmitPage {
        command: sdk::ConfigFormSubmitPage,
    },
    InvokeAction {
        command: sdk::ConfigFormInvokeAction,
    },
    Cancel {
        session_id: sdk::ConfigFormSessionId,
        revision: sdk::ConfigFormRevision,
    },
    Refresh {
        session_id: sdk::ConfigFormSessionId,
    },
}

pub(crate) struct ConfigFormModel {
    view: sdk::ConfigFormView,
    focused_field: usize,
    input: String,
    selected_option: usize,
    focused_action: usize,
    scroll: u16,
}

impl ConfigFormModel {
    pub(crate) fn new(view: sdk::ConfigFormView) -> Self {
        let input = initial_input(&view, 0);
        Self {
            view,
            focused_field: 0,
            input,
            selected_option: 0,
            focused_action: 0,
            scroll: 0,
        }
    }

    pub(crate) fn view(&self) -> &sdk::ConfigFormView {
        &self.view
    }

    pub(crate) fn replace_view(&mut self, view: sdk::ConfigFormView) {
        self.clear_sensitive_input();
        self.view = view;
        self.focused_field = 0;
        self.selected_option = 0;
        self.focused_action = 0;
        self.scroll = 0;
        self.input = initial_input(&self.view, self.focused_field);
    }

    pub(crate) fn interaction(&self) -> super::config_form_render::ConfigFormInteraction {
        super::config_form_render::ConfigFormInteraction {
            focused_field: self.focused_field,
            selected_option: self.selected_option,
            focused_action: self.focused_action,
        }
    }

    pub(crate) fn visible_input(&self) -> String {
        match self.focused_field_type() {
            Some(sdk::ConfigFormFieldType::Secret) => "•".repeat(self.input.chars().count()),
            _ => self.input.clone(),
        }
    }

    pub(crate) fn scroll(&self) -> u16 {
        self.scroll
    }

    pub(crate) fn update(&mut self, key: crossterm::event::KeyEvent) -> Option<ConfigFormEffect> {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc => Some(ConfigFormEffect::Cancel {
                session_id: self.view.session_id.clone(),
                revision: self.view.revision,
            }),
            KeyCode::Tab | KeyCode::Down => {
                self.focus_next();
                None
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.focus_previous();
                None
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(1);
                None
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(1);
                None
            }
            KeyCode::Left => {
                self.select_previous();
                None
            }
            KeyCode::Right => {
                self.select_next();
                None
            }
            KeyCode::Backspace => {
                self.input.pop();
                None
            }
            KeyCode::Char(character) => {
                if self.accepts_text_input() {
                    self.input.push(character);
                }
                None
            }
            KeyCode::Enter => self.submit_or_invoke(),
            _ => None,
        }
    }

    pub(crate) fn refresh_effect(&self) -> Option<ConfigFormEffect> {
        self.view.busy.as_ref().and_then(|busy| {
            matches!(
                busy.refresh_policy,
                sdk::ConfigFormRefreshPolicy::Poll { .. }
            )
            .then(|| ConfigFormEffect::Refresh {
                session_id: self.view.session_id.clone(),
            })
        })
    }

    fn submit_or_invoke(&mut self) -> Option<ConfigFormEffect> {
        if self.view.busy.is_some() || self.view.terminal.is_some() {
            return None;
        }
        if self.view.page.fields.is_empty() {
            return self
                .view
                .page
                .actions
                .get(self.focused_action)
                .map(|action| ConfigFormEffect::InvokeAction {
                    command: sdk::ConfigFormInvokeAction {
                        session_id: self.view.session_id.clone(),
                        expected_revision: self.view.revision,
                        action_id: action.id.clone(),
                    },
                });
        }
        let values = self.page_values()?;
        let effect = ConfigFormEffect::SubmitPage {
            command: sdk::ConfigFormSubmitPage {
                session_id: self.view.session_id.clone(),
                expected_revision: self.view.revision,
                values,
            },
        };
        self.clear_sensitive_input();
        Some(effect)
    }

    fn page_values(&self) -> Option<Vec<sdk::ConfigFormFieldValue>> {
        self.view
            .page
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| {
                !matches!(
                    field.field_type,
                    sdk::ConfigFormFieldType::Summary | sdk::ConfigFormFieldType::Status
                )
            })
            .map(|(index, field)| {
                let value = if index == self.focused_field {
                    self.input.clone()
                } else {
                    field.display_value.clone().unwrap_or_default()
                };
                let value = match field.field_type {
                    sdk::ConfigFormFieldType::Text => sdk::ConfigFormValue::Text(value),
                    sdk::ConfigFormFieldType::Secret => sdk::ConfigFormValue::Secret(value),
                    sdk::ConfigFormFieldType::Number => {
                        sdk::ConfigFormValue::Number(value.parse().ok()?)
                    }
                    sdk::ConfigFormFieldType::Boolean => {
                        sdk::ConfigFormValue::Boolean(parse_boolean(&value)?)
                    }
                    sdk::ConfigFormFieldType::SingleSelect => {
                        let option = field.options.get(self.selected_option)?;
                        sdk::ConfigFormValue::SelectedOption(option.id.clone())
                    }
                    sdk::ConfigFormFieldType::Summary | sdk::ConfigFormFieldType::Status => {
                        return None;
                    }
                };
                Some(sdk::ConfigFormFieldValue {
                    field_id: field.id.clone(),
                    value,
                })
            })
            .collect()
    }

    fn focused_field_type(&self) -> Option<sdk::ConfigFormFieldType> {
        self.view
            .page
            .fields
            .get(self.focused_field)
            .map(|field| field.field_type)
    }

    fn accepts_text_input(&self) -> bool {
        matches!(
            self.focused_field_type(),
            Some(
                sdk::ConfigFormFieldType::Text
                    | sdk::ConfigFormFieldType::Secret
                    | sdk::ConfigFormFieldType::Number
            )
        )
    }

    fn focus_next(&mut self) {
        if self.view.page.fields.is_empty() {
            if !self.view.page.actions.is_empty() {
                self.focused_action = (self.focused_action + 1) % self.view.page.actions.len();
            }
            return;
        }
        self.clear_sensitive_input();
        self.focused_field = (self.focused_field + 1) % self.view.page.fields.len();
        self.selected_option = 0;
        self.input = initial_input(&self.view, self.focused_field);
    }

    fn focus_previous(&mut self) {
        if self.view.page.fields.is_empty() {
            if !self.view.page.actions.is_empty() {
                self.focused_action = self
                    .focused_action
                    .checked_sub(1)
                    .unwrap_or(self.view.page.actions.len() - 1);
            }
            return;
        }
        self.clear_sensitive_input();
        self.focused_field = self
            .focused_field
            .checked_sub(1)
            .unwrap_or(self.view.page.fields.len() - 1);
        self.selected_option = 0;
        self.input = initial_input(&self.view, self.focused_field);
    }

    fn select_next(&mut self) {
        if self.view.page.fields.is_empty() {
            if !self.view.page.actions.is_empty() {
                self.focused_action = (self.focused_action + 1) % self.view.page.actions.len();
            }
            return;
        }
        let Some(field) = self.view.page.fields.get(self.focused_field) else {
            return;
        };
        if !field.options.is_empty() {
            self.selected_option = (self.selected_option + 1) % field.options.len();
        }
    }

    fn select_previous(&mut self) {
        if self.view.page.fields.is_empty() {
            if !self.view.page.actions.is_empty() {
                self.focused_action = self
                    .focused_action
                    .checked_sub(1)
                    .unwrap_or(self.view.page.actions.len() - 1);
            }
            return;
        }
        let Some(field) = self.view.page.fields.get(self.focused_field) else {
            return;
        };
        if !field.options.is_empty() {
            self.selected_option = self
                .selected_option
                .checked_sub(1)
                .unwrap_or(field.options.len() - 1);
        }
    }

    fn clear_sensitive_input(&mut self) {
        if matches!(
            self.focused_field_type(),
            Some(sdk::ConfigFormFieldType::Secret)
        ) {
            self.input.clear();
        }
    }
}

fn initial_input(view: &sdk::ConfigFormView, focused_field: usize) -> String {
    view.page
        .fields
        .get(focused_field)
        .filter(|field| field.field_type != sdk::ConfigFormFieldType::Secret)
        .and_then(|field| field.display_value.clone())
        .unwrap_or_default()
}

fn parse_boolean(value: &str) -> Option<bool> {
    match value.trim() {
        "是" | "true" | "y" | "yes" => Some(true),
        "否" | "false" | "n" | "no" => Some(false),
        _ => None,
    }
}

pub(crate) async fn execute_form_effect(
    client: &dyn sdk::ConfigFormClient,
    effect: ConfigFormEffect,
) -> Result<Option<sdk::ConfigFormView>, sdk::SdkError> {
    match effect {
        ConfigFormEffect::SubmitPage { command } => client.submit_page(command).await.map(Some),
        ConfigFormEffect::InvokeAction { command } => client.invoke_action(command).await.map(Some),
        ConfigFormEffect::Cancel {
            session_id,
            revision,
        } => client.cancel_form(session_id, revision).await.map(Some),
        ConfigFormEffect::Refresh { session_id } => client.refresh_form(session_id).await,
    }
}

pub(crate) async fn run_config_form(
    client: std::sync::Arc<dyn sdk::ConfigFormClient>,
    workflow_id: sdk::ConfigFormWorkflowId,
    origin: sdk::ConfigFormOrigin,
) -> Result<sdk::ConfigFormTerminal, sdk::SdkError> {
    use crossterm::event::{self, Event, KeyEventKind};
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::backend::CrosstermBackend;
    use ratatui::Terminal;
    use std::io;

    struct FormTerminalGuard {
        terminal: Terminal<CrosstermBackend<io::Stdout>>,
    }
    impl Drop for FormTerminalGuard {
        fn drop(&mut self) {
            let _ = disable_raw_mode();
            let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
            let _ = self.terminal.show_cursor();
        }
    }

    enable_raw_mode().map_err(|error| sdk::SdkError::Internal(error.to_string()))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|error| sdk::SdkError::Internal(error.to_string()))?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout))
        .map_err(|error| sdk::SdkError::Internal(error.to_string()))?;
    let mut guard = FormTerminalGuard { terminal };
    let view = client.start_form(workflow_id, origin).await?;
    let mut model = ConfigFormModel::new(view);

    loop {
        guard
            .terminal
            .draw(|frame| {
                super::config_form_render::render_config_form(
                    frame,
                    model.view(),
                    &model.visible_input(),
                    model.scroll(),
                    model.interaction(),
                )
            })
            .map_err(|error| sdk::SdkError::Internal(error.to_string()))?;
        if let Some(terminal) = model.view().terminal.clone() {
            return Ok(terminal);
        }
        if let Some(effect) = model.refresh_effect() {
            let view = execute_form_effect(client.as_ref(), effect)
                .await?
                .ok_or_else(|| sdk::SdkError::Internal("Config Form 会话不存在".to_string()))?;
            model.replace_view(view);
            continue;
        }
        let Event::Key(key) =
            event::read().map_err(|error| sdk::SdkError::Internal(error.to_string()))?
        else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let Some(effect) = model.update(key) else {
            continue;
        };
        let view = execute_form_effect(client.as_ref(), effect)
            .await?
            .ok_or_else(|| sdk::SdkError::Internal("Config Form 会话不存在".to_string()))?;
        model.replace_view(view);
    }
}

#[cfg(test)]
#[path = "config_form_tests.rs"]
mod tests;
