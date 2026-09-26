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
    /// 焦点是否在底部 action 区（Tab / ←→ 切换）；Enter 触发聚焦 action。
    focusing_actions: bool,
    /// MultiSelect 字段的勾选集合（字段索引 → 勾选的 option 索引集合）。
    multi_selection: std::collections::HashMap<usize, std::collections::HashSet<usize>>,
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
        let focused_action = initial_focused_action(&view);
        let multi_selection = multi_selection_from_view(&view);
        Self {
            view,
            focused_field: 0,
            input,
            field_inputs,
            input_cursor,
            selected_option: 0,
            focusing_actions: false,
            multi_selection,
            focused_action,
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
        self.selected_option = self.initial_selected_option();
        self.multi_selection = multi_selection_from_view(&self.view);
        self.focusing_actions = false;
        self.focused_action = initial_focused_action(&self.view);
        self.scroll = 0;
        self.field_inputs = initial_field_inputs(&self.view);
        self.input = self.field_inputs.first().cloned().unwrap_or_default();
        self.input_cursor = self.input.chars().count();
    }

    /// focused 字段为 SingleSelect 且携带已选值（has_value + display_value）
    /// 时，返回匹配 option 的索引；否则从 0 开始。用于进入/切换字段时把
    /// 全局配置预填的默认值直接高亮。
    fn initial_selected_option(&self) -> usize {
        let Some(field) = self.view.page.fields.get(self.focused_field) else {
            return 0;
        };
        if field.field_type != sdk::ConfigFormFieldType::SingleSelect || !field.has_value {
            return 0;
        }
        field
            .display_value
            .as_deref()
            .and_then(|selected| {
                field
                    .options
                    .iter()
                    .position(|option| option.label == selected)
            })
            .unwrap_or(0)
    }

    pub(crate) fn interaction(&self) -> super::config_form_render::ConfigFormInteraction {
        super::config_form_render::ConfigFormInteraction {
            focused_field: self.focused_field,
            selected_option: self.selected_option,
            focused_action: self.focused_action,
            focusing_actions: self.focusing_actions,
            input_cursor_column: self.input_cursor_column(),
            multi_selection: self.multi_selection.clone(),
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
            // MultiSelect：空格切换当前高亮 option 的勾选。
            KeyCode::Char(' ')
                if self.focused_field_type() == Some(sdk::ConfigFormFieldType::MultiSelect) =>
            {
                self.toggle_multi_selection();
                None
            }
            // Ctrl+U：清空当前输入框（终端惯例，便于覆盖长预填值）。
            KeyCode::Char('u')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                if self.accepts_text_input() {
                    self.input.clear();
                    self.input_cursor = 0;
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

    fn toggle_multi_selection(&mut self) {
        let field_index = self.focused_field;
        let selected = self.selected_option;
        let chosen = self.multi_selection.entry(field_index).or_default();
        if !chosen.insert(selected) {
            chosen.remove(&selected);
        }
    }

    fn save_focused_field_input(&mut self) {
        if let Some(field_input) = self.field_inputs.get_mut(self.focused_field) {
            *field_input = self.input.clone();
        }
    }

    fn current_field(&self) -> Option<&sdk::ConfigFormField> {
        self.view.page.fields.get(self.focused_field)
    }

    fn submit_or_invoke(&mut self) -> Option<ConfigFormEffect> {
        if self.view.busy.is_some() || self.view.terminal.is_some() {
            return None;
        }
        // 焦点在 action 区：回车直接触发聚焦 action（如"添加模型"）。
        if self.focusing_actions {
            let action = self.view.page.actions.get(self.focused_action)?.clone();
            let option_suffix = self
                .current_field()
                .filter(|field| {
                    matches!(
                        field.field_type,
                        sdk::ConfigFormFieldType::SingleSelect
                            | sdk::ConfigFormFieldType::MultiSelect
                    )
                })
                .and_then(|field| {
                    field
                        .options
                        .get(self.selected_option)
                        .map(|option| format!(":{}", option.id.as_str()))
                })
                .unwrap_or_default();
            self.focusing_actions = false;
            return Some(ConfigFormEffect::InvokeAction {
                command: sdk::ConfigFormInvokeAction {
                    session_id: self.view.session_id.clone(),
                    expected_revision: self.view.revision,
                    action_id: sdk::ConfigFormActionId(format!(
                        "{}{}",
                        action.id.as_str(),
                        option_suffix
                    )),
                },
            });
        }
        // 无可提交字段（fields 为空，或全部为 Summary / Status 只读字段，
        // 如 ConfirmOverwrite 摘要页）时，回车触发聚焦 action（覆盖 / 返回），
        // 而不是提交空字段集导致"页面不接受字段提交"。
        let has_submittable_field = self.view.page.fields.iter().any(|field| {
            !matches!(
                field.field_type,
                sdk::ConfigFormFieldType::Summary | sdk::ConfigFormFieldType::Status
            )
        });
        if !has_submittable_field {
            return self
                .view
                .page
                .actions
                .get(self.focused_action)
                .map(|action| {
                    // 附加当前高亮 option 作为参数后缀（如编辑目标
                    // enter_custom_model:configured-xxx）；业务侧解析主名，
                    // 无高亮 option 时不附加，其他 action 忽略后缀。
                    let option_suffix = self
                        .current_field()
                        .filter(|field| {
                            matches!(
                                field.field_type,
                                sdk::ConfigFormFieldType::SingleSelect
                                    | sdk::ConfigFormFieldType::MultiSelect
                            )
                        })
                        .and_then(|field| {
                            field
                                .options
                                .get(self.selected_option)
                                .map(|option| format!(":{}", option.id.as_str()))
                        })
                        .unwrap_or_default();
                    ConfigFormEffect::InvokeAction {
                        command: sdk::ConfigFormInvokeAction {
                            session_id: self.view.session_id.clone(),
                            expected_revision: self.view.revision,
                            action_id: sdk::ConfigFormActionId(format!(
                                "{}{}",
                                action.id.as_str(),
                                option_suffix
                            )),
                        },
                    }
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
                    sdk::ConfigFormFieldType::MultiSelect => {
                        let chosen = self
                            .multi_selection
                            .get(&index)
                            .cloned()
                            .unwrap_or_default();
                        let ids: Vec<sdk::ConfigFormOptionId> = field
                            .options
                            .iter()
                            .enumerate()
                            .filter(|(option_index, _)| chosen.contains(option_index))
                            .map(|(_, option)| option.id.clone())
                            .collect();
                        sdk::ConfigFormValue::SelectedOptions(ids)
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
        // Tab：字段区 ↔ action 区轮转；action 区内切下一个 action。
        if self.view.page.fields.is_empty() {
            if !self.view.page.actions.is_empty() {
                self.focused_action = (self.focused_action + 1) % self.view.page.actions.len();
            }
            return;
        }
        if self.focusing_actions {
            self.focused_action = (self.focused_action + 1) % self.view.page.actions.len();
        } else if self.view.page.fields.len() > 1 {
            self.focus_next_field();
        } else if !self.view.page.actions.is_empty() {
            self.focusing_actions = true;
        }
    }

    fn focus_previous_region(&mut self) {
        if self.view.page.fields.is_empty() {
            if !self.view.page.actions.is_empty() {
                self.focused_action = self
                    .focused_action
                    .checked_sub(1)
                    .unwrap_or(self.view.page.actions.len() - 1);
            }
            return;
        }
        if self.focusing_actions {
            self.focusing_actions = false;
        } else if self.view.page.fields.len() > 1 {
            self.focus_previous_field();
        }
    }

    /// ←→：action 区内切换；字段区非文本字段按 → 进入 action 区、← 回字段。
    fn focus_action_region(&mut self, forward: bool) {
        if self.view.page.actions.is_empty() {
            return;
        }
        // 无字段页 action 即唯一焦点，直接切换。
        if self.view.page.fields.is_empty() || self.focusing_actions {
            if forward {
                self.focused_action = (self.focused_action + 1) % self.view.page.actions.len();
            } else {
                self.focused_action = self
                    .focused_action
                    .checked_sub(1)
                    .unwrap_or(self.view.page.actions.len() - 1);
            }
        } else if !self.accepts_text_input() && forward {
            self.focusing_actions = true;
        }
    }

    fn navigate_down(&mut self) {
        if self.focusing_actions && !self.view.page.actions.is_empty() {
            self.focused_action = (self.focused_action + 1) % self.view.page.actions.len();
        } else if matches!(
            self.focused_field_type(),
            Some(sdk::ConfigFormFieldType::SingleSelect | sdk::ConfigFormFieldType::MultiSelect)
        ) {
            self.select_next();
        } else if self.view.page.fields.len() > 1 {
            self.focus_next_field();
        } else if self.view.page.fields.is_empty() {
            self.select_next();
        }
    }

    fn navigate_up(&mut self) {
        if self.focusing_actions && !self.view.page.actions.is_empty() {
            self.focused_action = self
                .focused_action
                .checked_sub(1)
                .unwrap_or(self.view.page.actions.len() - 1);
        } else if matches!(
            self.focused_field_type(),
            Some(sdk::ConfigFormFieldType::SingleSelect | sdk::ConfigFormFieldType::MultiSelect)
        ) {
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
        } else if self.focusing_actions {
            self.focus_action_region(false);
        } else if self.focused_field_type() == Some(sdk::ConfigFormFieldType::Boolean)
            || self.view.page.fields.is_empty()
        {
            self.select_previous();
        }
    }

    fn navigate_right(&mut self) {
        if self.accepts_text_input() {
            self.input_cursor = (self.input_cursor + 1).min(self.input.chars().count());
        } else if self.focusing_actions
            || (!self.view.page.actions.is_empty()
                && self.focused_field_type() != Some(sdk::ConfigFormFieldType::Boolean))
        {
            // →：进入 / 在 action 区内后移（Boolean 字段保留原切换语义）。
            self.focus_action_region(true);
        } else if self.view.page.fields.is_empty() {
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
        self.selected_option = self.initial_selected_option();
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
        self.selected_option = self.initial_selected_option();
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

/// action 驱动页（无字段）的初始焦点：指向 Primary 样式按钮（如
/// "测试连接" / "保存"），无 Primary 时回落第一个。否则回车默认触发
/// 第一个 action（如"跳过测试"），用户以为在测试实际被跳过。
/// MultiSelect 字段的初始勾选：display_value 存放已选 option label 的
/// 逗号分隔串（表单层生成）；据此恢复勾选索引。
fn multi_selection_from_view(
    view: &sdk::ConfigFormView,
) -> std::collections::HashMap<usize, std::collections::HashSet<usize>> {
    let mut selection = std::collections::HashMap::new();
    for (field_index, field) in view.page.fields.iter().enumerate() {
        if field.field_type != sdk::ConfigFormFieldType::MultiSelect {
            continue;
        }
        let Some(display) = field.display_value.as_deref() else {
            continue;
        };
        let chosen: std::collections::HashSet<&str> = display
            .split(',')
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .collect();
        let indexes: std::collections::HashSet<usize> = field
            .options
            .iter()
            .enumerate()
            .filter(|(_, option)| chosen.contains(option.label.as_str()))
            .map(|(index, _)| index)
            .collect();
        if !indexes.is_empty() {
            selection.insert(field_index, indexes);
        }
    }
    selection
}

fn initial_focused_action(view: &sdk::ConfigFormView) -> usize {
    view.page
        .actions
        .iter()
        .position(|action| action.style == sdk::ConfigFormActionStyle::Primary)
        .unwrap_or(0)
}

fn initial_field_inputs(view: &sdk::ConfigFormView) -> Vec<String> {
    // display_value（预填默认值/已保留凭证掩码）作为待编辑初值：
    // Secret 类型经 visible_input 以密码格式显示，掩码原样提交由表单层
    // 归一为空（保留现有 key）。
    view.page
        .fields
        .iter()
        .map(|field| field.display_value.clone().unwrap_or_default())
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
