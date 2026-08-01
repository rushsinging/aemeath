#[path = "service.rs"]
mod service;

pub use service::{ConfigFormService, SubmittedConfigFormPage};

use std::fmt;

use uuid::Uuid;

macro_rules! form_id {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ConfigFormError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(ConfigFormError::InvalidIdentifier);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($name))
                    .field(&self.0)
                    .finish()
            }
        }
    };
}

form_id!(ConfigFormWorkflowId);
form_id!(ConfigFormPageId);
form_id!(ConfigFormFieldId);
form_id!(ConfigFormOptionId);
form_id!(ConfigFormActionId);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConfigFormSessionId(String);

impl ConfigFormSessionId {
    pub(crate) fn next() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConfigFormRevision(u64);

impl ConfigFormRevision {
    pub const fn initial() -> Self {
        Self(0)
    }

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub(crate) const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormOrigin {
    ExplicitCommand,
    FirstChatBootstrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormFieldType {
    Text,
    Secret,
    Number,
    SingleSelect,
    Boolean,
    Summary,
    Status,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ConfigFormValue {
    Text(String),
    Secret(String),
    Number(u64),
    Boolean(bool),
    SelectedOption(ConfigFormOptionId),
}

impl ConfigFormValue {
    pub(crate) fn field_type(&self) -> ConfigFormFieldType {
        match self {
            Self::Text(_) => ConfigFormFieldType::Text,
            Self::Secret(_) => ConfigFormFieldType::Secret,
            Self::Number(_) => ConfigFormFieldType::Number,
            Self::Boolean(_) => ConfigFormFieldType::Boolean,
            Self::SelectedOption(_) => ConfigFormFieldType::SingleSelect,
        }
    }
}

impl fmt::Debug for ConfigFormValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(value) => formatter.debug_tuple("Text").field(value).finish(),
            Self::Secret(_) => formatter.write_str("Secret([REDACTED])"),
            Self::Number(value) => formatter.debug_tuple("Number").field(value).finish(),
            Self::Boolean(value) => formatter.debug_tuple("Boolean").field(value).finish(),
            Self::SelectedOption(value) => formatter
                .debug_tuple("SelectedOption")
                .field(value)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFormFieldValue {
    pub field_id: ConfigFormFieldId,
    pub value: ConfigFormValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFormOption {
    pub id: ConfigFormOptionId,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFormFieldError {
    pub field_id: ConfigFormFieldId,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFormField {
    pub id: ConfigFormFieldId,
    pub label: String,
    pub description: Option<String>,
    pub field_type: ConfigFormFieldType,
    pub required: bool,
    pub has_value: bool,
    pub display_value: Option<String>,
    pub options: Vec<ConfigFormOption>,
    pub error: Option<ConfigFormFieldError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigFormStep {
    pub current: u16,
    pub total: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormActionStyle {
    Primary,
    Secondary,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFormAction {
    pub id: ConfigFormActionId,
    pub label: String,
    pub style: ConfigFormActionStyle,
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFormPageError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFormPage {
    pub id: ConfigFormPageId,
    pub title: String,
    pub description: Option<String>,
    pub step: Option<ConfigFormStep>,
    pub fields: Vec<ConfigFormField>,
    pub error: Option<ConfigFormPageError>,
    pub actions: Vec<ConfigFormAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormRefreshPolicy {
    Manual,
    Poll { interval_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFormBusy {
    pub message: String,
    pub cancellable: bool,
    pub refresh_policy: ConfigFormRefreshPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFormTerminal {
    Completed { applied_revision: Option<u64> },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFormView {
    pub workflow_id: ConfigFormWorkflowId,
    pub session_id: ConfigFormSessionId,
    pub revision: ConfigFormRevision,
    pub origin: ConfigFormOrigin,
    pub page: ConfigFormPage,
    pub busy: Option<ConfigFormBusy>,
    pub terminal: Option<ConfigFormTerminal>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ConfigFormCommand {
    SubmitPage { values: Vec<ConfigFormFieldValue> },
    InvokeAction { action_id: ConfigFormActionId },
    Back,
    Cancel,
    Refresh,
}

impl fmt::Debug for ConfigFormCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubmitPage { values } => formatter
                .debug_struct("SubmitPage")
                .field("field_count", &values.len())
                .finish(),
            Self::InvokeAction { action_id } => formatter
                .debug_struct("InvokeAction")
                .field("action_id", action_id)
                .finish(),
            Self::Back => formatter.write_str("Back"),
            Self::Cancel => formatter.write_str("Cancel"),
            Self::Refresh => formatter.write_str("Refresh"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFormError {
    InvalidIdentifier,
    SessionNotFound,
    StaleRevision {
        actual: ConfigFormRevision,
        provided: ConfigFormRevision,
    },
    UnknownField {
        field_id: ConfigFormFieldId,
    },
    DuplicateField {
        field_id: ConfigFormFieldId,
    },
    MissingRequiredField {
        field_id: ConfigFormFieldId,
    },
    InvalidValueType {
        field_id: ConfigFormFieldId,
        expected: ConfigFormFieldType,
        actual: ConfigFormFieldType,
    },
    UnknownOption {
        field_id: ConfigFormFieldId,
        option_id: ConfigFormOptionId,
    },
    ReadOnlyField {
        field_id: ConfigFormFieldId,
    },
    UnknownAction {
        action_id: ConfigFormActionId,
    },
}

impl ConfigFormError {
    pub fn display_message(&self) -> String {
        match self {
            Self::InvalidIdentifier => "Config Form 标识符不能为空".to_string(),
            Self::SessionNotFound => "Config Form 会话不存在".to_string(),
            Self::StaleRevision { .. } => "Config Form revision 已变更，请刷新后重试".to_string(),
            Self::UnknownField { field_id } => format!("未知字段：{}", field_id.as_str()),
            Self::DuplicateField { field_id } => format!("字段重复提交：{}", field_id.as_str()),
            Self::MissingRequiredField { field_id } => {
                format!("缺少必填字段：{}", field_id.as_str())
            }
            Self::InvalidValueType { field_id, .. } => {
                format!("字段类型不匹配：{}", field_id.as_str())
            }
            Self::UnknownOption { field_id, .. } => format!("字段选项无效：{}", field_id.as_str()),
            Self::ReadOnlyField { field_id } => format!("字段只读：{}", field_id.as_str()),
            Self::UnknownAction { action_id } => format!("未知动作：{}", action_id.as_str()),
        }
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod service_tests;
