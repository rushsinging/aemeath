//! Pure Config DTOs exposed by the SDK.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFieldData {
    Model,
    PermissionMode,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigChangeCauseData {
    ClientUpdate,
    ProjectCommit,
    FileReload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionModeView {
    Ask,
    AutoRead,
    AllowAll,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MarkdownSpacingModeView {
    #[default]
    Normal,
    Compact,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ElementSpacingView {
    #[serde(default)]
    pub before: Option<u8>,
    #[serde(default)]
    pub after: Option<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MarkdownSpacingOverridesView {
    #[serde(default)]
    pub paragraph: Option<ElementSpacingView>,
    #[serde(default)]
    pub heading: Option<ElementSpacingView>,
    #[serde(default)]
    pub list: Option<ElementSpacingView>,
    #[serde(default)]
    pub code_block: Option<ElementSpacingView>,
    #[serde(default)]
    pub table: Option<ElementSpacingView>,
    #[serde(default)]
    pub blockquote: Option<ElementSpacingView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigView {
    pub model_name: String,
    pub provider: Option<String>,
    pub has_api_key: bool,
    pub permission_mode: String,
    pub markdown: bool,
    pub verbose: bool,
    #[serde(default)]
    pub markdown_spacing: MarkdownSpacingModeView,
    #[serde(default)]
    pub markdown_spacing_overrides: MarkdownSpacingOverridesView,
    pub context_size: usize,
    pub logging_level: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigUpdateData {
    SetModel { model: String },
    SetPermissionMode { mode: PermissionModeView },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigUpdateResult {
    pub changed_fields: Vec<ConfigFieldData>,
    pub view: ConfigView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigApplicationScopeView {
    Immediate,
    SessionRestartRequired,
    Run,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigReloadedEvent {
    pub changed_keys: Vec<String>,
    pub scopes: Vec<ConfigApplicationScopeView>,
    #[serde(default)]
    pub view: ConfigView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigChangedEvent {
    pub cause: ConfigChangeCauseData,
    pub changed_fields: Vec<ConfigFieldData>,
    pub view: ConfigView,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_update_round_trips_as_typed_command() {
        let update = ConfigUpdateData::SetPermissionMode {
            mode: PermissionModeView::AllowAll,
        };
        let json = serde_json::to_string(&update).unwrap();
        assert_eq!(
            serde_json::from_str::<ConfigUpdateData>(&json).unwrap(),
            update
        );
    }
}
