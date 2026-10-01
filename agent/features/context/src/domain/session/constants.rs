//! session 子域常量（#1146 双轨归位）。

pub(crate) const CURRENT_SESSION_SCHEMA_VERSION: u32 = 6;

pub const CURRENT_SESSION_GENERATION_SCHEMA_VERSION: u32 = 1;
pub(crate) const MANIFEST_MEMBER_NAME: &str = "manifest.json";
pub(crate) const SESSION_STATE_MEMBER_NAME: &str = "session-state.json";
pub(crate) const SESSION_METADATA_MEMBER_NAME: &str = "metadata.json";
pub(crate) const SESSION_TASK_MEMBER_NAME: &str = "task-state.json";
pub(crate) const SESSION_WORKSPACE_MEMBER_NAME: &str = "workspace-state.json";
pub(crate) const SESSION_RECEIPT_MEMBER_NAME: &str = "receipt-ledger.json";
pub(crate) const SESSION_SKILL_MEMBER_NAME: &str = "skill-loads.json";
