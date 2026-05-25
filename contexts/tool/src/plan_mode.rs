//! Enter/Exit Plan Mode tools
//!
//! Plan mode allows the agent to create a detailed plan before executing actions.
//! In plan mode, tool calls are only simulated and not actually executed.

use async_trait::async_trait;
use kernel::tool::{Tool, ToolContext, ToolResult};
use serde::{Deserialize, Serialize};

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
impl Tool for EnterPlanModeTool {
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

    async fn call(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let _args: PlanModeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => return ToolResult::error(format!("Invalid input: {}", e)),
        };

        // Set plan mode in context
        if let Some(mode) = ctx.plan_mode.as_ref() {
            if *mode {
                return ToolResult::success("Already in plan mode".to_string());
            }
        }

        // Note: The actual mode change happens in the agent runner
        ToolResult::success(
            "Entered plan mode. Tool calls will be simulated and not executed. \
             Use ExitPlanMode to return to normal execution mode."
                .to_string(),
        )
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

#[async_trait]
impl Tool for ExitPlanModeTool {
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

    async fn call(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let args: ExitPlanModeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => return ToolResult::error(format!("Invalid input: {}", e)),
        };

        // Check if we're in plan mode
        if let Some(mode) = ctx.plan_mode.as_ref() {
            if !*mode {
                return ToolResult::success("Not in plan mode".to_string());
            }
        }

        if args.execute {
            ToolResult::success(
                "Exited plan mode. The planned actions will now be executed. \
                 Note: Simulated tool calls need to be re-invoked."
                    .to_string(),
            )
        } else {
            ToolResult::success(
                "Exited plan mode. Returning to normal execution without running planned actions."
                    .to_string(),
            )
        }
    }

    fn is_read_only(&self) -> bool {
        true
    }
}
