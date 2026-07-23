//! Pure Config DTOs exposed by the SDK.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigField {
    Model,
    PermissionMode,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigChangeCause {
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigView {
    pub model_name: String,
    pub provider: Option<String>,
    pub has_api_key: bool,
    pub permission_mode: String,
    pub markdown: bool,
    pub verbose: bool,
    pub context_size: usize,
    pub logging_level: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigUpdate {
    SetModel { model: String },
    SetPermissionMode { mode: PermissionModeView },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigUpdateResult {
    pub changed_fields: Vec<ConfigField>,
    pub view: ConfigView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigApplicationScopeView {
    SessionRestartRequired,
    Run,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigReloadedEvent {
    pub changed_keys: Vec<String>,
    pub scopes: Vec<ConfigApplicationScopeView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigChangedEvent {
    pub cause: ConfigChangeCause,
    pub changed_fields: Vec<ConfigField>,
    pub view: ConfigView,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_update_round_trips_as_typed_command() {
        let update = ConfigUpdate::SetPermissionMode {
            mode: PermissionModeView::AllowAll,
        };
        let json = serde_json::to_string(&update).unwrap();
        assert_eq!(serde_json::from_str::<ConfigUpdate>(&json).unwrap(), update);
    }
}
