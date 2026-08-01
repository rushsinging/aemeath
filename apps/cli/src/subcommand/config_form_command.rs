#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigFormEffect {
    SubmitPage {
        command: sdk::ConfigFormSubmitPage,
    },
    InvokeAction {
        command: sdk::ConfigFormInvokeAction,
    },
    Back {
        session_id: sdk::ConfigFormSessionId,
        revision: sdk::ConfigFormRevision,
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
    field_inputs: Vec<String>,
    input_cursor: usize,
    selected_option: usize,
    focused_action: usize,
    scroll: u16,
}

impl ConfigFormModel {
    pub(crate) fn new(view: sdk::ConfigFormView) -> Self {
        let input = initial_field_inputs(&view)
            .first()
            .cloned()
            .unwrap_or_default();
        let input_cursor = input.chars().count();
        let field_inputs = initial_field_inputs(&view);
        Self {
            view,
            focused_field: 0,
            input,
            field_inputs,
            input_cursor,
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
        self.field_inputs = initial_field_inputs(&self.view);
        self.input = self.field_inputs.first().cloned().unwrap_or_default();
        self.input_cursor = self.input.chars().count();
    }

    pub(crate) fn interaction(&self) -> super::config_form_render::ConfigFormInteraction {
        super::config_form_render::ConfigFormInteraction {
            focused_field: self.focused_field,
            selected_option: self.selected_option,
            focused_action: self.focused_action,
            input_cursor_column: self.input_cursor_column(),
        }
    }

    pub(crate) fn visible_input(&self) -> String {
        match self.focused_field_type() {
            Some(sdk::ConfigFormFieldType::Secret) => "•".repeat(self.input.chars().count()),
            _ => self.input.clone(),
        }
    }

    pub(crate) fn input_cursor_column(&self) -> Option<usize> {
        self.accepts_text_input().then_some(self.input_cursor)
    }

    pub(crate) fn scroll(&self) -> u16 {
        self.scroll
    }

    pub(crate) fn update(&mut self, key: crossterm::event::KeyEvent) -> Option<ConfigFormEffect> {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Esc => Some(if self.is_initial_page() {
                ConfigFormEffect::Cancel {
                    session_id: self.view.session_id.clone(),
                    revision: self.view.revision,
                }
            } else {
                ConfigFormEffect::Back {
                    session_id: self.view.session_id.clone(),
                    revision: self.view.revision,
                }
            }),
            KeyCode::Tab => {
                self.focus_next_region();
                None
            }
            KeyCode::BackTab => {
                self.focus_previous_region();
                None
            }
            KeyCode::Down => {
                self.navigate_down();
                None
            }
            KeyCode::Up => {
                self.navigate_up();
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
                self.navigate_left();
                None
            }
            KeyCode::Right => {
                self.navigate_right();
                None
            }
            KeyCode::Backspace => {
                self.delete_before_cursor();
                None
            }
            KeyCode::Delete => {
                self.delete_at_cursor();
                None
            }
            KeyCode::Home => {
                if self.accepts_text_input() {
                    self.input_cursor = 0;
                }
                None
            }
            KeyCode::End => {
                if self.accepts_text_input() {
                    self.input_cursor = self.input.chars().count();
                }
                None
            }
            KeyCode::Char(character) => {
                if self.accepts_text_input() {
                    insert_character(&mut self.input, self.input_cursor, character);
                    self.input_cursor += 1;
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

    fn save_focused_field_input(&mut self) {
        if let Some(field_input) = self.field_inputs.get_mut(self.focused_field) {
            *field_input = self.input.clone();
        }
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
        self.save_focused_field_input();
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
                let value = self
                    .field_inputs
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| field.display_value.clone().unwrap_or_default());
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

    fn is_initial_page(&self) -> bool {
        self.view.page.id.as_str() == "select_provider"
    }

    fn focus_next_region(&mut self) {
        if self.view.page.fields.len() > 1 {
            self.focus_next_field();
        } else if self.view.page.fields.is_empty() && self.view.page.actions.len() > 1 {
            self.focused_action = (self.focused_action + 1) % self.view.page.actions.len();
        }
    }

    fn focus_previous_region(&mut self) {
        if self.view.page.fields.len() > 1 {
            self.focus_previous_field();
        } else if self.view.page.fields.is_empty() && self.view.page.actions.len() > 1 {
            self.focused_action = self
                .focused_action
                .checked_sub(1)
                .unwrap_or(self.view.page.actions.len() - 1);
        }
    }

    fn navigate_down(&mut self) {
        if self.focused_field_type() == Some(sdk::ConfigFormFieldType::SingleSelect) {
            self.select_next();
        } else if self.view.page.fields.len() > 1 {
            self.focus_next_field();
        } else if self.view.page.fields.is_empty() {
            self.select_next();
        }
    }

    fn navigate_up(&mut self) {
        if self.focused_field_type() == Some(sdk::ConfigFormFieldType::SingleSelect) {
            self.select_previous();
        } else if self.view.page.fields.len() > 1 {
            self.focus_previous_field();
        } else if self.view.page.fields.is_empty() {
            self.select_previous();
        }
    }

    fn navigate_left(&mut self) {
        if self.accepts_text_input() {
            self.input_cursor = self.input_cursor.saturating_sub(1);
        } else if self.focused_field_type() == Some(sdk::ConfigFormFieldType::Boolean)
            || self.view.page.fields.is_empty()
        {
            self.select_previous();
        }
    }

    fn navigate_right(&mut self) {
        if self.accepts_text_input() {
            self.input_cursor = (self.input_cursor + 1).min(self.input.chars().count());
        } else if self.focused_field_type() == Some(sdk::ConfigFormFieldType::Boolean)
            || self.view.page.fields.is_empty()
        {
            self.select_next();
        }
    }

    fn delete_before_cursor(&mut self) {
        if !self.accepts_text_input() || self.input_cursor == 0 {
            return;
        }
        self.input_cursor -= 1;
        remove_character(&mut self.input, self.input_cursor);
    }

    fn delete_at_cursor(&mut self) {
        if self.accepts_text_input() {
            remove_character(&mut self.input, self.input_cursor);
        }
    }

    fn focus_next_field(&mut self) {
        if self.view.page.fields.is_empty() {
            if !self.view.page.actions.is_empty() {
                self.focused_action = (self.focused_action + 1) % self.view.page.actions.len();
            }
            return;
        }
        self.save_focused_field_input();
        self.clear_sensitive_input();
        self.focused_field = (self.focused_field + 1) % self.view.page.fields.len();
        self.selected_option = 0;
        self.input = self
            .field_inputs
            .get(self.focused_field)
            .cloned()
            .unwrap_or_default();
        self.input_cursor = self.input.chars().count();
    }

    fn focus_previous_field(&mut self) {
        if self.view.page.fields.is_empty() {
            if !self.view.page.actions.is_empty() {
                self.focused_action = self
                    .focused_action
                    .checked_sub(1)
                    .unwrap_or(self.view.page.actions.len() - 1);
            }
            return;
        }
        self.save_focused_field_input();
        self.clear_sensitive_input();
        self.focused_field = self
            .focused_field
            .checked_sub(1)
            .unwrap_or(self.view.page.fields.len() - 1);
        self.selected_option = 0;
        self.input = self
            .field_inputs
            .get(self.focused_field)
            .cloned()
            .unwrap_or_default();
        self.input_cursor = self.input.chars().count();
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
            if let Some(field_input) = self.field_inputs.get_mut(self.focused_field) {
                field_input.clear();
            }
        }
    }
}

fn character_byte_index(value: &str, character_index: usize) -> usize {
    value
        .char_indices()
        .nth(character_index)
        .map_or(value.len(), |(byte_index, _)| byte_index)
}

fn insert_character(value: &mut String, character_index: usize, character: char) {
    let byte_index = character_byte_index(value, character_index);
    value.insert(byte_index, character);
}

fn remove_character(value: &mut String, character_index: usize) {
    let start = character_byte_index(value, character_index);
    let end = character_byte_index(value, character_index + 1);
    if start < end {
        value.replace_range(start..end, "");
    }
}

fn initial_field_inputs(view: &sdk::ConfigFormView) -> Vec<String> {
    view.page
        .fields
        .iter()
        .map(|field| {
            if field.field_type == sdk::ConfigFormFieldType::Secret {
                String::new()
            } else {
                field.display_value.clone().unwrap_or_default()
            }
        })
        .collect()
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
        ConfigFormEffect::Back {
            session_id,
            revision,
        } => client.back_form(session_id, revision).await.map(Some),
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
