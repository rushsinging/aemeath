use crate::catalog::{find_by_source, ProviderCatalogEntry, ProviderSource};
use crate::connect::{AvailableAction, ConnectCommand, ConnectOutcome};
use crate::connect::{ConnectStage, ConnectView, ProbeStatusView};

use super::*;

pub const PROVIDER_CONNECT_WORKFLOW_ID: &str = "provider_connect";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderConnectFormError {
    Form(ConfigFormError),
    UnknownProvider(String),
    InvalidSubmission(String),
}

impl std::fmt::Display for ProviderConnectFormError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Form(error) => formatter.write_str(&error.display_message()),
            Self::UnknownProvider(source) => write!(formatter, "未知 Provider：{source}"),
            Self::InvalidSubmission(message) => formatter.write_str(message),
        }
    }
}

impl From<ConfigFormError> for ProviderConnectFormError {
    fn from(error: ConfigFormError) -> Self {
        Self::Form(error)
    }
}

pub fn provider_connect_form_view(
    connect: &ConnectView,
    catalog: &'static [ProviderCatalogEntry],
) -> Result<ConfigFormView, ProviderConnectFormError> {
    Ok(ConfigFormView {
        workflow_id: ConfigFormWorkflowId::new(PROVIDER_CONNECT_WORKFLOW_ID)?,
        session_id: ConfigFormSessionId(connect.session_id.to_transport_string()),
        revision: ConfigFormRevision::new(connect.revision.value()),
        origin: match connect.origin {
            crate::connect::ConnectOrigin::ExplicitCommand => ConfigFormOrigin::ExplicitCommand,
            crate::connect::ConnectOrigin::FirstChatBootstrap => {
                ConfigFormOrigin::FirstChatBootstrap
            }
        },
        page: page_for_connect(connect, catalog)?,
        busy: busy_for_connect(connect),
        terminal: connect.terminal.clone().map(|outcome| match outcome {
            ConnectOutcome::Completed { applied_revision } => ConfigFormTerminal::Completed {
                applied_revision: Some(applied_revision),
            },
            ConnectOutcome::Cancelled => ConfigFormTerminal::Cancelled,
        }),
    })
}

pub fn connect_command_for_form(
    connect: &ConnectView,
    command: ConfigFormCommand,
    catalog: &'static [ProviderCatalogEntry],
) -> Result<ConnectCommand, ProviderConnectFormError> {
    match command {
        ConfigFormCommand::SubmitPage { values } => submit_for_stage(connect, catalog, values),
        ConfigFormCommand::InvokeAction { action_id } => action_for_id(action_id.as_str()),
        ConfigFormCommand::Cancel => Err(ProviderConnectFormError::InvalidSubmission(
            "取消应调用 Connect cancel 入口".to_string(),
        )),
        ConfigFormCommand::Back => Ok(ConnectCommand::Back),
        ConfigFormCommand::Refresh => Err(ProviderConnectFormError::InvalidSubmission(
            "刷新不产生 Connect 命令".to_string(),
        )),
    }
}

fn page_for_connect(
    connect: &ConnectView,
    catalog: &'static [ProviderCatalogEntry],
) -> Result<ConfigFormPage, ConfigFormError> {
    let (id, title, fields) = match connect.stage {
        ConnectStage::SelectProvider => {
            let mut options: Vec<ConfigFormOption> = catalog
                .iter()
                .map(|entry| {
                    option(
                        entry.source.as_str(),
                        entry.source.as_str(),
                        Some(entry.driver.as_str()),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            // 全局配置已有但不在 catalog 的自定义 Provider（如 OmniRoute 等）
            // 也进入列表，选中后走覆盖确认与已有值预填，而不是强迫用户
            // 经"自定义"重新输入全部信息。
            for existing in &connect.existing_providers {
                if !catalog
                    .iter()
                    .any(|entry| entry.source.as_str() == existing.source)
                {
                    options.push(option(
                        &existing.source,
                        &existing.source,
                        existing.driver.as_deref(),
                    )?);
                }
            }
            options.push(option("custom", "自定义（完全自定义）", None)?);
            (
                "select_provider",
                "选择 Provider",
                vec![select_field("provider_source", "Provider", options)?],
            )
        }
        ConnectStage::EditCustomProvider => {
            // 完全自定义：名称 / driver / endpoint 全手填，无 catalog 默认。
            let mut driver_ids: Vec<&str> = Vec::new();
            for entry in catalog {
                if !driver_ids.contains(&entry.driver.as_str()) {
                    driver_ids.push(entry.driver.as_str());
                }
            }
            (
                "edit_custom_provider",
                "自定义 Provider",
                vec![
                    text_field("provider_name", "Provider 名称", true, None)?,
                    select_field(
                        "custom_driver",
                        "Driver",
                        driver_ids
                            .into_iter()
                            .map(|driver| option(driver, driver, None))
                            .collect::<Result<Vec<_>, _>>()?,
                    )?,
                    text_field("custom_base_url", "Base URL", true, None)?,
                ],
            )
        }
        ConnectStage::ConfirmOverwrite => (
            "confirm_overwrite",
            "确认覆盖 Provider",
            vec![summary_field(
                "existing_provider",
                "现有 Provider",
                connect
                    .existing_provider
                    .as_ref()
                    .map(|provider| provider.source.clone())
                    .unwrap_or_else(|| "已存在".to_string()),
            )?],
        ),
        ConnectStage::EditEndpoint => {
            let mut fields = vec![text_field(
                "base_url",
                "Base URL",
                true,
                connect.draft.base_url.clone().or_else(|| {
                    connect
                        .draft
                        .source
                        .as_ref()
                        .and_then(|source| catalog.iter().find(|entry| entry.source == *source))
                        .and_then(|entry| {
                            entry
                                .default_endpoint
                                .as_ref()
                                .map(|endpoint| endpoint.url.to_string())
                        })
                }),
            )?];
            // OpenAI 系 driver 提供 Chat Completions / Responses 接口风格
            // 选择；anthropic / ollama 仅支持 Chat，不显示该字段。driver
            // 优先取 draft，缺失时由 catalog source 推导。
            let resolved_driver = connect.draft.driver.map(|driver| driver).or_else(|| {
                connect
                    .draft
                    .source
                    .as_ref()
                    .and_then(|source| catalog.iter().find(|entry| entry.source == *source))
                    .map(|entry| entry.driver)
            });
            if let Some(driver) = resolved_driver {
                if supports_responses_api(driver.as_str()) {
                    let mut style_field = select_field(
                        "api_style",
                        "接口风格",
                        vec![
                            option("chat", "Chat Completions", None)?,
                            option("responses", "Responses", None)?,
                        ],
                    )?;
                    let label = if connect.draft.api_style.as_deref() == Some("responses") {
                        "Responses"
                    } else {
                        "Chat Completions"
                    };
                    style_field.has_value = true;
                    style_field.display_value = Some(label.to_string());
                    fields.push(style_field);
                }
            }
            ("edit_endpoint", "设置 Base URL", fields)
        }
        ConnectStage::EditCredential => {
            let mut field = secret_field("api_key", "API Key", connect.draft.has_api_key)?;
            if let Some(mask) = connect.draft.credential_mask.as_deref() {
                field.display_value = Some(mask.to_string());
                field.description = Some(format!(
                    "已加载现有密钥 {mask}（密码格式）；直接回车保留，输入新值覆盖"
                ));
            }
            ("edit_credential", "设置 API Key", vec![field])
        }
        ConnectStage::EditUserAgent => (
            "edit_user_agent",
            "设置 Provider User-Agent",
            vec![text_field(
                "provider_user_agent",
                "User-Agent",
                false,
                connect.draft.provider_user_agent.clone().or_else(|| {
                    connect
                        .draft
                        .source
                        .as_ref()
                        .and_then(|source| catalog.iter().find(|entry| entry.source == *source))
                        .and_then(|entry| {
                            entry
                                .official_sdk_user_agent
                                .as_ref()
                                .map(|official| official.value.to_string())
                        })
                }),
            )?],
        ),
        ConnectStage::SelectModel => (
            "select_model",
            "选择模型",
            vec![recommended_models_field(connect, catalog)?],
        ),
        ConnectStage::EditCustomModel => {
            // 预填编辑目标（模型页高亮项，经 action 后缀传入）；
            // 无目标时回落首个已选模型；纯添加场景用户直接覆盖输入。
            let editing = connect
                .draft
                .editing_model_id
                .as_deref()
                .and_then(|target| {
                    connect
                        .draft
                        .models
                        .iter()
                        .find(|model| model.model_id == target)
                })
                .or_else(|| connect.draft.models.first());
            let mut effort_field = select_field(
                "reasoning_effort",
                "Reasoning Effort",
                vec![
                    option("default", "默认（全局）", None)?,
                    option("off", "off", None)?,
                    option("minimal", "minimal", None)?,
                    option("low", "low", None)?,
                    option("medium", "medium", None)?,
                    option("high", "high", None)?,
                    option("xhigh", "xhigh", None)?,
                    option("max", "max", None)?,
                ],
            )?;
            if let Some(effort) = editing.and_then(|model| model.reasoning_effort.as_deref()) {
                if effort_field
                    .options
                    .iter()
                    .any(|option| option.id.as_str() == effort)
                {
                    effort_field.has_value = true;
                    effort_field.display_value = Some(effort.to_string());
                }
            }
            (
                "edit_custom_model",
                "添加 / 编辑模型",
                vec![
                    text_field(
                        "model_id",
                        "Model ID",
                        true,
                        editing.map(|model| model.model_id.clone()),
                    )?,
                    number_field(
                        "context_window",
                        "Context Window",
                        true,
                        editing
                            .and_then(|model| model.context_window)
                            .and_then(|value| u64::try_from(value).ok()),
                    )?,
                    number_field(
                        "max_tokens",
                        "Max Tokens",
                        true,
                        editing.and_then(|model| model.max_tokens).map(u64::from),
                    )?,
                    effort_field,
                ],
            )
        }
        ConnectStage::ChooseGlobalDefault => {
            let mut field = select_field(
                "set_global_default",
                "设为全局默认",
                vec![option("yes", "是", None)?, option("no", "否", None)?],
            )?;
            let label = if connect.draft.set_global_default {
                "是"
            } else {
                "否"
            };
            field.has_value = true;
            field.display_value = Some(label.to_string());
            ("choose_global_default", "设置全局默认模型", vec![field])
        }
        ConnectStage::ChooseProbe => ("choose_probe", "测试连接", Vec::new()),
        ConnectStage::Probing => (
            "probe_status",
            "连接测试",
            vec![status_field(
                "probe_status",
                "状态",
                probe_status_text(connect.probe_status.as_ref()),
            )?],
        ),
        ConnectStage::Review => ("review", "检查并保存", review_fields(connect)?),
        ConnectStage::Saving => (
            "saving",
            "保存配置",
            vec![status_field(
                "saving_status",
                "状态",
                "正在保存".to_string(),
            )?],
        ),
        ConnectStage::Completed => (
            "completed",
            "配置已保存",
            vec![status_field(
                "completed_status",
                "状态",
                "已完成".to_string(),
            )?],
        ),
        ConnectStage::Cancelled => (
            "cancelled",
            "配置已取消",
            vec![status_field(
                "cancelled_status",
                "状态",
                "已取消".to_string(),
            )?],
        ),
    };
    Ok(ConfigFormPage {
        id: ConfigFormPageId::new(id)?,
        title: title.to_string(),
        description: None,
        step: step_for_stage(connect.stage),
        fields,
        error: connect
            .last_error
            .as_ref()
            .map(|error| ConfigFormPageError {
                message: error.display_message(),
            }),
        actions: actions_for_connect(connect)?,
    })
}

fn submit_for_stage(
    connect: &ConnectView,
    catalog: &'static [ProviderCatalogEntry],
    values: Vec<ConfigFormFieldValue>,
) -> Result<ConnectCommand, ProviderConnectFormError> {
    let stage = connect.stage;
    let field = |id: &str| {
        values
            .iter()
            .find(|value| value.field_id.as_str() == id)
            .ok_or_else(|| ProviderConnectFormError::InvalidSubmission(format!("缺少字段：{id}")))
    };
    Ok(match stage {
        ConnectStage::SelectProvider => {
            let ConfigFormValue::SelectedOption(option_id) = &field("provider_source")?.value
            else {
                return Err(invalid_type("provider_source"));
            };
            if option_id.as_str() == "custom" {
                return Ok(ConnectCommand::BeginCustomProvider);
            }
            // catalog 内条目复用静态 source；catalog 外的已有自定义 source
            //（列表来自 existing 快照）用 new_owned 持有运行时名称。
            let source = find_by_source(option_id.as_str())
                .map(|entry| entry.source.clone())
                .unwrap_or_else(|| ProviderSource::new_owned(option_id.as_str().to_string()));
            ConnectCommand::SelectProvider { source }
        }
        ConnectStage::EditCustomProvider => {
            let name = text_value(field("provider_name")?, "provider_name")?;
            let ConfigFormValue::SelectedOption(option_id) = &field("custom_driver")?.value else {
                return Err(invalid_type("custom_driver"));
            };
            let base_url = text_value(field("custom_base_url")?, "custom_base_url")?;
            ConnectCommand::SelectCustomProvider {
                name,
                driver: option_id.as_str().to_string(),
                base_url,
            }
        }
        ConnectStage::EditEndpoint => {
            let base_url = text_value(field("base_url")?, "base_url")?;
            let resolved_driver = connect.draft.driver.clone().or_else(|| {
                connect
                    .draft
                    .source
                    .as_ref()
                    .and_then(|source| catalog.iter().find(|entry| entry.source == *source))
                    .map(|entry| entry.driver)
            });
            let api_style = if resolved_driver
                .map(|driver| supports_responses_api(driver.as_str()))
                .unwrap_or(false)
            {
                let ConfigFormValue::SelectedOption(option_id) = &field("api_style")?.value else {
                    return Err(invalid_type("api_style"));
                };
                match option_id.as_str() {
                    "chat" => None,
                    "responses" => Some("responses".to_string()),
                    _ => return Err(invalid_value("api_style")),
                }
            } else {
                None
            };
            ConnectCommand::SetEndpoint {
                base_url,
                api_style,
            }
        }
        ConnectStage::EditCredential => {
            let mut api_key = secret_value(field("api_key")?, "api_key")?;
            // 掩码原样提交（用户未改动预填值）归一为空提交，保留现有 key。
            if let Some(mask) = connect.draft.credential_mask.as_deref() {
                if api_key == mask {
                    api_key.clear();
                }
            }
            ConnectCommand::SetCredential { api_key }
        }
        ConnectStage::EditUserAgent => {
            let value = text_value(field("provider_user_agent")?, "provider_user_agent")?;
            ConnectCommand::SetProviderUserAgent {
                raw: (!value.trim().is_empty()).then_some(value),
            }
        }
        ConnectStage::SelectModel => {
            let ConfigFormValue::SelectedOptions(option_ids) = &field("recommended_models")?.value
            else {
                return Err(invalid_type("recommended_models"));
            };
            let mut models = Vec::new();
            for option_id in option_ids {
                let Some((model_id, context_window, max_tokens)) =
                    model_spec_for_option_id(connect, catalog, option_id.as_str())
                else {
                    return Err(invalid_value("recommended_models"));
                };
                let reasoning_effort = connect
                    .draft
                    .models
                    .iter()
                    .find(|model| model.model_id == model_id)
                    .and_then(|model| model.reasoning_effort.clone());
                models.push(crate::connect::ModelDraft {
                    model_id,
                    context_window,
                    max_tokens,
                    reasoning_effort,
                });
            }
            ConnectCommand::SetSelectedModels { models }
        }
        ConnectStage::EditCustomModel => {
            let reasoning_effort = {
                let ConfigFormValue::SelectedOption(option_id) = &field("reasoning_effort")?.value
                else {
                    return Err(invalid_type("reasoning_effort"));
                };
                match option_id.as_str() {
                    "default" => None,
                    value => Some(value.to_string()),
                }
            };
            ConnectCommand::UpsertCustomModel {
                model: crate::connect::ModelDraft {
                    model_id: text_value(field("model_id")?, "model_id")?,
                    context_window: usize::try_from(number_value(
                        field("context_window")?,
                        "context_window",
                    )?)
                    .map_err(|_| invalid_value("context_window"))?,
                    max_tokens: u32::try_from(number_value(field("max_tokens")?, "max_tokens")?)
                        .map_err(|_| invalid_value("max_tokens"))?,
                    reasoning_effort,
                },
            }
        }
        ConnectStage::ChooseGlobalDefault => {
            let ConfigFormValue::SelectedOption(option_id) = &field("set_global_default")?.value
            else {
                return Err(invalid_type("set_global_default"));
            };
            let set_as_default = match option_id.as_str() {
                "yes" => true,
                "no" => false,
                _ => return Err(invalid_value("set_global_default")),
            };
            ConnectCommand::SetGlobalDefault { set_as_default }
        }
        // 探测完成（含失败）后回车提交 = 继续到 Review；页面 actions 仍可
        // 返回编辑或取消。
        ConnectStage::Probing => ConnectCommand::ContinueAfterProbe,
        _ => {
            return Err(ProviderConnectFormError::InvalidSubmission(format!(
                "{stage:?} 页面不接受字段提交"
            )))
        }
    })
}

fn action_for_id(action_id: &str) -> Result<ConnectCommand, ProviderConnectFormError> {
    Ok(match action_id {
        "confirm_overwrite" => ConnectCommand::ConfirmOverwrite,
        "reject_overwrite" => ConnectCommand::RejectOverwrite,
        "skip_probe" => ConnectCommand::SkipProbe,
        "begin_probe" => ConnectCommand::BeginProbe,
        "continue_after_probe" => ConnectCommand::ContinueAfterProbe,
        "edit_after_probe_failure" => ConnectCommand::EditAfterProbeFailure,
        "confirm_save" | "retry_save" => ConnectCommand::ConfirmSave,
        // 支持 `{name}:{option_id}` 后缀（TUI 附带高亮 option 作参数）。
        raw => {
            let (main, target) = raw.split_once(':').unwrap_or((raw, ""));
            match main {
                "enter_custom_model" => ConnectCommand::EnterCustomModel {
                    target_model: (!target.is_empty()).then(|| {
                        target
                            .strip_prefix("configured-")
                            .unwrap_or(target)
                            .to_string()
                    }),
                },
                _ => {
                    return Err(ProviderConnectFormError::InvalidSubmission(format!(
                        "未知动作：{action_id}"
                    )))
                }
            }
        }
    })
}

fn actions_for_connect(connect: &ConnectView) -> Result<Vec<ConfigFormAction>, ConfigFormError> {
    connect
        .available_actions
        .iter()
        .filter_map(|action| action_schema(*action))
        .map(|(id, label, style)| {
            Ok(ConfigFormAction {
                id: ConfigFormActionId::new(id)?,
                label: label.to_string(),
                style,
                shortcut: None,
            })
        })
        .collect()
}

fn action_schema(
    action: AvailableAction,
) -> Option<(&'static str, &'static str, ConfigFormActionStyle)> {
    let secondary = ConfigFormActionStyle::Secondary;
    let primary = ConfigFormActionStyle::Primary;
    let destructive = ConfigFormActionStyle::Destructive;
    Some(match action {
        AvailableAction::SelectProvider
        | AvailableAction::SetEndpoint
        | AvailableAction::SetCredential
        | AvailableAction::SetProviderUserAgent
        | AvailableAction::SelectRecommendedModel
        | AvailableAction::SetCustomModel
        | AvailableAction::SetGlobalDefault
        | AvailableAction::BeginCustomProvider
        | AvailableAction::SelectCustomProvider => return None,
        // 模型多选页的"添加 / 编辑模型"入口（可无限次进入 upsert）。
        AvailableAction::EnterCustomModel => ("enter_custom_model", "添加 / 编辑模型", secondary),
        AvailableAction::ConfirmOverwrite => ("confirm_overwrite", "覆盖", primary),
        AvailableAction::RejectOverwrite => ("reject_overwrite", "返回", secondary),
        AvailableAction::SkipProbe => ("skip_probe", "跳过测试", secondary),
        AvailableAction::BeginProbe => ("begin_probe", "测试连接", primary),
        AvailableAction::ContinueAfterProbe => ("continue_after_probe", "继续", primary),
        AvailableAction::EditAfterProbeFailure => {
            ("edit_after_probe_failure", "返回编辑", secondary)
        }
        AvailableAction::ConfirmSave => ("confirm_save", "保存", primary),
        AvailableAction::RetrySave => ("retry_save", "重试保存", primary),
        AvailableAction::Cancel => ("cancel", "取消", destructive),
    })
}

fn busy_for_connect(connect: &ConnectView) -> Option<ConfigFormBusy> {
    match connect.stage {
        ConnectStage::Probing if matches!(connect.probe_status, Some(ProbeStatusView::Running)) => {
            Some(ConfigFormBusy {
                message: "正在测试连接".to_string(),
                cancellable: true,
                refresh_policy: ConfigFormRefreshPolicy::Poll { interval_ms: 100 },
            })
        }
        ConnectStage::Saving => Some(ConfigFormBusy {
            message: "正在保存配置".to_string(),
            cancellable: true,
            refresh_policy: ConfigFormRefreshPolicy::Poll { interval_ms: 100 },
        }),
        _ => None,
    }
}

/// 判断 driver 是否支持 Responses API 接口风格。与 provider crate 的
/// `driver_acl` 保持同一规则：anthropic / ollama 仅 Chat，其余 OpenAI 系
/// 均支持 Responses。
fn supports_responses_api(driver: &str) -> bool {
    !driver.eq_ignore_ascii_case("anthropic") && !driver.eq_ignore_ascii_case("ollama")
}

fn select_field(
    id: &str,
    label: &str,
    options: Vec<ConfigFormOption>,
) -> Result<ConfigFormField, ConfigFormError> {
    Ok(ConfigFormField {
        id: ConfigFormFieldId::new(id)?,
        label: label.to_string(),
        description: None,
        field_type: ConfigFormFieldType::SingleSelect,
        required: true,
        has_value: false,
        display_value: None,
        options,
        error: None,
    })
}

fn text_field(
    id: &str,
    label: &str,
    required: bool,
    value: Option<String>,
) -> Result<ConfigFormField, ConfigFormError> {
    Ok(ConfigFormField {
        id: ConfigFormFieldId::new(id)?,
        label: label.to_string(),
        description: None,
        field_type: ConfigFormFieldType::Text,
        required,
        has_value: value.as_ref().is_some_and(|value| !value.is_empty()),
        display_value: value,
        options: Vec::new(),
        error: None,
    })
}

fn secret_field(
    id: &str,
    label: &str,
    has_value: bool,
) -> Result<ConfigFormField, ConfigFormError> {
    Ok(ConfigFormField {
        id: ConfigFormFieldId::new(id)?,
        label: label.to_string(),
        description: None,
        field_type: ConfigFormFieldType::Secret,
        required: false,
        has_value,
        display_value: None,
        options: Vec::new(),
        error: None,
    })
}

fn number_field(
    id: &str,
    label: &str,
    required: bool,
    value: Option<u64>,
) -> Result<ConfigFormField, ConfigFormError> {
    Ok(ConfigFormField {
        id: ConfigFormFieldId::new(id)?,
        label: label.to_string(),
        description: None,
        field_type: ConfigFormFieldType::Number,
        required,
        has_value: value.is_some(),
        display_value: value.map(|value| value.to_string()),
        options: Vec::new(),
        error: None,
    })
}

fn summary_field(id: &str, label: &str, value: String) -> Result<ConfigFormField, ConfigFormError> {
    read_only_field(id, label, value, ConfigFormFieldType::Summary)
}

fn status_field(id: &str, label: &str, value: String) -> Result<ConfigFormField, ConfigFormError> {
    read_only_field(id, label, value, ConfigFormFieldType::Status)
}

fn read_only_field(
    id: &str,
    label: &str,
    value: String,
    field_type: ConfigFormFieldType,
) -> Result<ConfigFormField, ConfigFormError> {
    Ok(ConfigFormField {
        id: ConfigFormFieldId::new(id)?,
        label: label.to_string(),
        description: None,
        field_type,
        required: false,
        has_value: true,
        display_value: Some(value),
        options: Vec::new(),
        error: None,
    })
}

fn option(
    id: &str,
    label: &str,
    description: Option<&str>,
) -> Result<ConfigFormOption, ConfigFormError> {
    Ok(ConfigFormOption {
        id: ConfigFormOptionId::new(id)?,
        label: label.to_string(),
        description: description.map(str::to_string),
    })
}

/// 模型多选字段：推荐模型 ∪ 已配置模型（含自定义），已配置默认勾选。
/// 空格切换勾选，回车提交完整集合。
fn recommended_models_field(
    connect: &ConnectView,
    catalog: &'static [ProviderCatalogEntry],
) -> Result<ConfigFormField, ConfigFormError> {
    let mut options: Vec<ConfigFormOption> = Vec::new();
    let mut chosen_labels: Vec<String> = Vec::new();
    let entry = connect
        .draft
        .source
        .as_ref()
        .and_then(|source| catalog.iter().find(|entry| entry.source == *source));
    if let Some(entry) = entry {
        for model in entry.recommended_models {
            options.push(option(
                &format!("recommended-{}", model.model_id),
                model.model_id,
                Some(&format!(
                    "Context {} · Max {}",
                    model.context_window, model.max_tokens
                )),
            )?);
        }
    }
    // 已配置模型（含推荐外的自定义）进入列表并默认勾选。
    for model in &connect.draft.models {
        let label = model.model_id.as_str();
        if !options.iter().any(|existing| existing.label == label) {
            let detail = format!(
                "Context {} · Max {}{}",
                model.context_window.unwrap_or(0),
                model.max_tokens.unwrap_or(0),
                model
                    .reasoning_effort
                    .as_deref()
                    .map(|effort| format!(" · {effort}"))
                    .unwrap_or_default(),
            );
            options.push(option(
                &format!("configured-{}", label),
                label,
                Some(&detail),
            )?);
        }
        chosen_labels.push(label.to_string());
    }
    Ok(ConfigFormField {
        id: ConfigFormFieldId::new("recommended_models")?,
        label: "模型（空格勾选，可多选）".to_string(),
        description: None,
        field_type: ConfigFormFieldType::MultiSelect,
        required: !options.is_empty(),
        has_value: !chosen_labels.is_empty(),
        display_value: (!chosen_labels.is_empty()).then(|| chosen_labels.join(", ")),
        options,
        error: None,
    })
}

/// 按 option id 解析模型规格（推荐命中 catalog，已配置命中 draft.models）。
fn model_spec_for_option_id(
    connect: &ConnectView,
    catalog: &'static [ProviderCatalogEntry],
    option_id: &str,
) -> Option<(String, usize, u32)> {
    if let Some(model_id) = option_id.strip_prefix("recommended-") {
        let entry = connect
            .draft
            .source
            .as_ref()
            .and_then(|source| catalog.iter().find(|entry| entry.source == *source))?;
        let model = entry
            .recommended_models
            .iter()
            .find(|model| model.model_id == model_id)?;
        return Some((
            model.model_id.to_string(),
            model.context_window,
            model.max_tokens,
        ));
    }
    if let Some(model_id) = option_id.strip_prefix("configured-") {
        let model = connect
            .draft
            .models
            .iter()
            .find(|model| model.model_id == model_id)?;
        return Some((
            model.model_id.clone(),
            model.context_window.unwrap_or(0),
            model.max_tokens.unwrap_or(0),
        ));
    }
    None
}

fn review_fields(connect: &ConnectView) -> Result<Vec<ConfigFormField>, ConfigFormError> {
    let mut fields = Vec::new();
    fields.push(summary_field(
        "review_provider",
        "Provider",
        connect
            .draft
            .source
            .as_ref()
            .map(|source| source.as_str().to_string())
            .unwrap_or_else(|| "未选择".to_string()),
    )?);
    fields.push(summary_field(
        "review_endpoint",
        "Base URL",
        connect
            .draft
            .base_url
            .clone()
            .unwrap_or_else(|| "未设置".to_string()),
    )?);
    if connect.draft.api_style.is_some() {
        fields.push(summary_field(
            "review_api_style",
            "接口风格",
            "Responses".to_string(),
        )?);
    }
    fields.push(summary_field(
        "review_credential",
        "API Key",
        if connect.draft.has_api_key {
            connect
                .draft
                .credential_mask
                .clone()
                .unwrap_or_else(|| "已设置".to_string())
        } else {
            "未设置".to_string()
        },
    )?);
    fields.push(summary_field(
        "review_models",
        "模型",
        if connect.draft.models.is_empty() {
            "未选择".to_string()
        } else {
            connect
                .draft
                .models
                .iter()
                .map(|model| {
                    format!(
                        "{}（Context {} · Max {}{}）",
                        model.model_id,
                        model.context_window.unwrap_or(0),
                        model.max_tokens.unwrap_or(0),
                        model
                            .reasoning_effort
                            .as_deref()
                            .map(|effort| format!(" · {effort}"))
                            .unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>()
                .join("、")
        },
    )?);
    fields.push(summary_field(
        "review_user_agent",
        "User-Agent",
        connect
            .draft
            .provider_user_agent
            .clone()
            .unwrap_or_else(|| "使用全局默认".to_string()),
    )?);
    fields.push(summary_field(
        "review_global_default",
        "设为全局默认",
        if connect.draft.set_global_default {
            "是".to_string()
        } else {
            "否".to_string()
        },
    )?);
    Ok(fields)
}

fn step_for_stage(stage: ConnectStage) -> Option<ConfigFormStep> {
    let current = match stage {
        ConnectStage::SelectProvider | ConnectStage::ConfirmOverwrite => 1,
        ConnectStage::EditCustomProvider | ConnectStage::EditEndpoint => 2,
        ConnectStage::EditCredential => 3,
        ConnectStage::EditUserAgent => 4,
        ConnectStage::SelectModel | ConnectStage::EditCustomModel => 5,
        ConnectStage::ChooseGlobalDefault => 6,
        ConnectStage::ChooseProbe | ConnectStage::Probing => 7,
        ConnectStage::Review | ConnectStage::Saving => 8,
        ConnectStage::Completed | ConnectStage::Cancelled => return None,
    };
    Some(ConfigFormStep { current, total: 8 })
}

fn probe_status_text(status: Option<&ProbeStatusView>) -> String {
    match status {
        None | Some(ProbeStatusView::NotRun) => "未测试".to_string(),
        Some(ProbeStatusView::Running) => "测试中".to_string(),
        Some(ProbeStatusView::Success { latency_ms }) => format!("成功（{latency_ms} ms）"),
        Some(ProbeStatusView::Failed { message, .. }) => format!("失败：{message}"),
    }
}

fn text_value(value: &ConfigFormFieldValue, id: &str) -> Result<String, ProviderConnectFormError> {
    match &value.value {
        ConfigFormValue::Text(value) => Ok(value.clone()),
        _ => Err(invalid_type(id)),
    }
}

fn secret_value(
    value: &ConfigFormFieldValue,
    id: &str,
) -> Result<String, ProviderConnectFormError> {
    match &value.value {
        ConfigFormValue::Secret(value) => Ok(value.clone()),
        _ => Err(invalid_type(id)),
    }
}

fn number_value(value: &ConfigFormFieldValue, id: &str) -> Result<u64, ProviderConnectFormError> {
    match value.value {
        ConfigFormValue::Number(value) => Ok(value),
        _ => Err(invalid_type(id)),
    }
}

fn invalid_type(id: &str) -> ProviderConnectFormError {
    ProviderConnectFormError::InvalidSubmission(format!("字段类型不匹配：{id}"))
}

fn invalid_value(id: &str) -> ProviderConnectFormError {
    ProviderConnectFormError::InvalidSubmission(format!("字段值无效：{id}"))
}
