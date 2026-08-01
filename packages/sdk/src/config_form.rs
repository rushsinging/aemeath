use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

string_id!(ConfigFormWorkflowId);
string_id!(ConfigFormSessionId);
string_id!(ConfigFormPageId);
string_id!(ConfigFormFieldId);
string_id!(ConfigFormOptionId);
string_id!(ConfigFormActionId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ConfigFormRevision(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormOrigin {
    ExplicitCommand,
    FirstChatBootstrap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormFieldType {
    Text,
    Secret,
    Number,
    SingleSelect,
    Boolean,
    Summary,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ConfigFormValue {
    Text(String),
    Secret(String),
    Number(u64),
    Boolean(bool),
    SelectedOption(ConfigFormOptionId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormFieldValue {
    pub field_id: ConfigFormFieldId,
    pub value: ConfigFormValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormOption {
    pub id: ConfigFormOptionId,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormFieldError {
    pub field_id: ConfigFormFieldId,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormStep {
    pub current: u16,
    pub total: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormActionStyle {
    Primary,
    Secondary,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormAction {
    pub id: ConfigFormActionId,
    pub label: String,
    pub style: ConfigFormActionStyle,
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormPageError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormPage {
    pub id: ConfigFormPageId,
    pub title: String,
    pub description: Option<String>,
    pub step: Option<ConfigFormStep>,
    pub fields: Vec<ConfigFormField>,
    pub error: Option<ConfigFormPageError>,
    pub actions: Vec<ConfigFormAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ConfigFormRefreshPolicy {
    Manual,
    Poll { interval_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormBusy {
    pub message: String,
    pub cancellable: bool,
    pub refresh_policy: ConfigFormRefreshPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormTerminal {
    Completed { applied_revision: Option<u64> },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormView {
    pub workflow_id: ConfigFormWorkflowId,
    pub session_id: ConfigFormSessionId,
    pub revision: ConfigFormRevision,
    pub origin: ConfigFormOrigin,
    pub page: ConfigFormPage,
    pub busy: Option<ConfigFormBusy>,
    pub terminal: Option<ConfigFormTerminal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormSubmitPage {
    pub session_id: ConfigFormSessionId,
    pub expected_revision: ConfigFormRevision,
    pub values: Vec<ConfigFormFieldValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormInvokeAction {
    pub session_id: ConfigFormSessionId,
    pub expected_revision: ConfigFormRevision,
    pub action_id: ConfigFormActionId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormErrorKind {
    InvalidIdentifier,
    UnknownWorkflow,
    SessionNotFound,
    StaleRevision,
    InvalidField,
    InvalidAction,
    Validation,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigFormErrorView {
    pub kind: ConfigFormErrorKind,
    pub message: String,
}
