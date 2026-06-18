//! Tool result adapters — runtime layer that converts `serde_json::Value`
//! back to the typed R struct defined in `packages/sdk/src/tool_result/`.
//!
//! See `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`
//! Phase 0a for the full design.
use serde::de::DeserializeOwned;
use sdk::tool_result::{
    ReadResult,
    WriteResult,
    EditResult,
    GlobResult,
    GrepResult,
    WebFetchResult,
    WebSearchResult,
    BashResult,
    SleepResult,
    AgentResult,
    AskUserResult,
    EnterWorktreeResult,
    ExitWorktreeResult,
    BriefResult,
    ConfigToolResult,
    LspResult,
    PlanModeResult,
    MemoryResult,
    SkillResult,
    TaskCreateResult,
    TaskGetResult,
    TaskListResult,
    TaskStopResult,
    TaskUpdateResult,
    TaskListCreateResult,
    TaskListCompleteResult,
    ToolSearchResult,
    McpToolResult,
    McpManagerResult,
    ListMcpResourcesResult,
    ReadMcpResourceResult,
};

/// Tool result adapter: convert `serde_json::Value` back to the typed R struct.
pub trait ToolResultAdapter: DeserializeOwned + Default + Sized {
    fn from_raw(data: &serde_json::Value) -> Self {
        serde_json::from_value(data.clone()).unwrap_or_default()
    }
}

impl ToolResultAdapter for ReadResult {}
impl ToolResultAdapter for WriteResult {}
impl ToolResultAdapter for EditResult {}
impl ToolResultAdapter for GlobResult {}
impl ToolResultAdapter for GrepResult {}
impl ToolResultAdapter for WebFetchResult {}
impl ToolResultAdapter for WebSearchResult {}
impl ToolResultAdapter for BashResult {}
impl ToolResultAdapter for SleepResult {}
impl ToolResultAdapter for AgentResult {}
impl ToolResultAdapter for AskUserResult {}
impl ToolResultAdapter for EnterWorktreeResult {}
impl ToolResultAdapter for ExitWorktreeResult {}
impl ToolResultAdapter for BriefResult {}
impl ToolResultAdapter for ConfigToolResult {}
impl ToolResultAdapter for LspResult {}
impl ToolResultAdapter for PlanModeResult {}
impl ToolResultAdapter for MemoryResult {}
impl ToolResultAdapter for SkillResult {}
impl ToolResultAdapter for TaskCreateResult {}
impl ToolResultAdapter for TaskGetResult {}
impl ToolResultAdapter for TaskListResult {}
impl ToolResultAdapter for TaskStopResult {}
impl ToolResultAdapter for TaskUpdateResult {}
impl ToolResultAdapter for TaskListCreateResult {}
impl ToolResultAdapter for TaskListCompleteResult {}
impl ToolResultAdapter for ToolSearchResult {}
impl ToolResultAdapter for McpToolResult {}
impl ToolResultAdapter for McpManagerResult {}
impl ToolResultAdapter for ListMcpResourcesResult {}
impl ToolResultAdapter for ReadMcpResourceResult {}

