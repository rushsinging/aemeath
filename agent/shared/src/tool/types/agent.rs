//! Typed result for the `agent` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `agent` tool (sub-agent dispatch).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AgentResult {
    /// 关联的 Task ID（仅当 agent 调用时传入了 taskId 才有值）。
    /// `#[serde(alias)]` 兼容旧字段名 `agent_id` 的反序列化。
    #[serde(default, alias = "agent_id")]
    pub task_id: Option<String>,
    pub output: String,
}

/// Typed input for the `agent` tool (sub-agent dispatch).
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// 字段标识符即 JSON property 名（build.rs 使用标识符，忽略 serde rename），
/// 因此 `taskId` 必须保留 camelCase 标识符并 `#[allow(non_snake_case)]`。
#[derive(Debug, Clone, Deserialize, Default)]
#[allow(non_snake_case)]
pub struct AgentInput {
    /// The task for the agent to perform
    pub prompt: String,
    /// A short (3-5 word) description of the task
    pub description: String,
    /// Agent role name defined in config (e.g. 'coder', 'reviewer'). Resolves to the model and settings configured for that role.
    pub role: Option<String>,
    /// Direct model override in 'provider/model_id' format (e.g. 'deepseek/deepseek-chat', 'ollama/llama3.2'). Takes precedence over 'role' if both are specified.
    pub model: Option<String>,
    /// Maximum number of tool-call rounds (max 1000)
    pub max_turns: Option<u64>,
    /// Task ID from TaskCreate. OPTIONAL — only pass when you want the dispatcher to auto-manage task status (InProgress on start, Completed on success, Pending on failure). Free-form exploration or ad-hoc agent calls do NOT need a taskId.
    pub taskId: Option<String>,
}
