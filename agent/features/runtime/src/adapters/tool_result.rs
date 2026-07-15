//! Tool result adapters — runtime layer that converts `serde_json::Value`
//! back to the typed R struct defined in `packages/sdk/src/tool_result/`.
//!
//! See `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`
//! Phase 0a for the full design.
use serde::de::DeserializeOwned;
use share::tool::types::{
    AgentResult, AskUserQuestionResult, BashResult, BriefResult, EditResult, EnterWorktreeResult,
    ExitWorktreeResult, GlobResult, GrepResult, ListMcpResourcesResult, LspResult,
    McpManagerResult, McpToolResult, MemoryResult, PlanModeResult, ReadMcpResourceResult,
    ReadResult, SkillResult, TaskCreateResult, TaskGetResult, TaskListCompleteResult,
    TaskListCreateResult, TaskListResult, TaskStopResult, TaskUpdateResult, ToolSearchResult,
    WebFetchResult, WebSearchResult, WriteResult,
};

/// Tool result adapter: convert `serde_json::Value` back to the typed R struct.
///
/// The trait deliberately does **not** require `Default` so that R structs
/// with non-`Default` inner types (e.g. `TaskListResult` whose element type
/// is `Task`, and `TaskGetResult` whose element is `Task`) can still
/// participate. Callers that want a "best-effort" decode should `.unwrap_or`
/// the error or fall back to a typed empty value themselves.
pub trait ToolResultAdapter: DeserializeOwned + Sized {
    fn from_raw(data: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(data.clone())
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
impl ToolResultAdapter for AgentResult {}
impl ToolResultAdapter for AskUserQuestionResult {}
impl ToolResultAdapter for EnterWorktreeResult {}
impl ToolResultAdapter for ExitWorktreeResult {}
impl ToolResultAdapter for BriefResult {}
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
