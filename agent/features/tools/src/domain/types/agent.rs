//! Typed result for the `agent` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `agent` tool (sub-agent dispatch).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AgentResult {
    pub output: String,
}

/// Typed input for the `agent` tool (sub-agent dispatch).
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// 字段标识符即 JSON property 名（build.rs 使用标识符，忽略 serde rename）。
///
/// `timeout` **MUST NOT** 在此声明：框架级通用参数，catalog 注入发布、
/// 执行前框架摘除，工具只读 `ctx.effective_timeout_secs()`。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentInput {
    /// The task for the agent to perform
    pub prompt: String,
    /// A short (3-5 word) description of the task
    pub description: String,
    /// Required named agent. Must exactly match an instance name in the system prompt's Available Agent Roles roster (machine key: `config.agents.names`).
    pub agent: String,
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
