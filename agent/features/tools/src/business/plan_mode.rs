//! Enter/Exit Plan Mode tools
//!
//! Plan mode allows the agent to create a detailed plan before executing actions.
//! In plan mode, tool calls are only simulated and not actually executed.

use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use share::tool::types::plan_mode::PlanModeResult;

/// Tool to enter plan mode
pub struct EnterPlanModeTool;

/// Tool to exit plan mode
pub struct ExitPlanModeTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanModeInput {
    /// Optional reason for entering plan mode
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitPlanModeInput {
    /// Whether to execute the planned actions
    #[serde(default)]
    pub execute: bool,
}

#[async_trait]
impl TypedTool for EnterPlanModeTool {
    type Output = PlanModeResult;
    fn name(&self) -> &'static str {
        "EnterPlanMode"
    }

    fn description(&self) -> &'static str {
        "Enter plan mode. In plan mode, tool calls are simulated and not actually executed. \
         Use this when you need to create a detailed plan before taking actions."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Optional reason for entering plan mode"
                }
            },
            "required": []
        })
    }

    async fn call(
        &self,
        input: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<PlanModeResult> {
        let _args: PlanModeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => return TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("Invalid input: {}", e), "data": null}).to_string()),
        };

        // Set plan mode in context
        if let Some(mode) = ctx.plan_mode.as_ref() {
            if *mode {
                return TypedToolResult::success_msg(serde_json::json!({"status": "success", "message": "Already in plan mode", "data": null}).to_string());
            }
        }

        // Note: The actual mode change happens in the agent runner
        TypedToolResult::success_msg(
            serde_json::json!({"status": "success", "message": "Entered plan mode. Tool calls will be simulated and not executed. Use ExitPlanMode to return to normal execution mode.", "data": null}).to_string(),
        )
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

#[async_trait]
impl TypedTool for ExitPlanModeTool {
    type Output = PlanModeResult;
    fn name(&self) -> &'static str {
        "ExitPlanMode"
    }

    fn description(&self) -> &'static str {
        "Exit plan mode and return to normal execution. \
         Optionally execute the planned actions that were simulated."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "execute": {
                    "type": "boolean",
                    "description": "Whether to execute the planned actions",
                    "default": false
                }
            },
            "required": []
        })
    }

    async fn call(
        &self,
        input: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<PlanModeResult> {
        let args: ExitPlanModeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => return TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("Invalid input: {}", e), "data": null}).to_string()),
        };

        // Check if we're in plan mode
        if let Some(mode) = ctx.plan_mode.as_ref() {
            if !*mode {
                return TypedToolResult::success_msg(serde_json::json!({"status": "success", "message": "Not in plan mode", "data": null}).to_string());
            }
        }

        if args.execute {
            TypedToolResult::success_msg(
                serde_json::json!({"status": "success", "message": "Exited plan mode. The planned actions will now be executed. Note: Simulated tool calls need to be re-invoked.", "data": {"execute": true}}).to_string(),
            )
        } else {
            TypedToolResult::success_msg(
                serde_json::json!({"status": "success", "message": "Exited plan mode. Returning to normal execution without running planned actions.", "data": {"execute": false}}).to_string(),
            )
        }
    }

    fn is_read_only(&self) -> bool {
        true
    }
}
