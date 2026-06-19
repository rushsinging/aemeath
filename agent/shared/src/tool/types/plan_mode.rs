//! Typed result for the `plan_mode` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `plan_mode` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PlanModeResult {
    pub reason: String,
    pub execute: Option<bool>,
}

/// Typed input for the `enter_plan_mode` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct EnterPlanModeInput {
    /// Optional reason for entering plan mode
    pub reason: Option<String>,
}

/// Typed input for the `exit_plan_mode` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExitPlanModeInput {
    /// Whether to execute the planned actions
    pub execute: Option<bool>,
}
