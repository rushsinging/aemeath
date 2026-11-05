//! Session-owned value types and identifiers.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::{NoContext, Timestamp, Uuid};

/// Validate a session ID to prevent path traversal attacks.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct SessionMetadata {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub is_favorite: bool,
    pub model: Option<String>,
    /// 会话选定的 reasoning effort 档位（"off".."max"，canonical 取值见
    /// `share::reasoning::ReasoningLevel::as_str`）。`None` 表示未显式选择，
    /// 跟随模型 entry 默认档。
    pub reasoning_effort: Option<String>,
    pub project: Option<String>,
    /// Session 来源（#2160 P2）：`None` / `Main` 为用户会话；`SubRun` 为 Agent
    /// 派生的子 Run 存档。`SubRun` **不进** session 列表枚举。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<SessionOrigin>,
    /// sub run 的父 session id（`origin = SubRun` 时有值）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    /// sub run 的父 Run id（`origin = SubRun` 时有值）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    /// sub run 归属的 Workflow / Node / attempt 槽位（P3 由 workflow adaptor 填充）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
}

/// Session 来源分类（#2160 P2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOrigin {
    /// 用户会话（默认；旧数据缺字段按此处理）。
    Main,
    /// Agent 派生的 sub run 存档：完整 canonical 落盘，但不参与 session 枚举。
    SubRun,
}

pub fn extract_project_name(cwd: &str) -> Option<String> {
    PathBuf::from(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

pub fn new_session_id() -> String {
    Uuid::new_v7(Timestamp::now(NoContext)).to_string()
}
