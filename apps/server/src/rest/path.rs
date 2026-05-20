use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkspacePath {
    pub workspace_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChatPath {
    pub workspace_id: String,
    pub chat_id: String,
}
