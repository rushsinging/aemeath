//! Enter/Exit Plan Mode tools
//!
//! Plan mode allows the agent to create a detailed plan before executing actions.
//! In plan mode, tool calls are only simulated and not actually executed.

use crate::domain::types::plan_mode::{EnterPlanModeInput, ExitPlanModeInput, PlanModeResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;

/// Tool to enter plan mode
pub struct EnterPlanModeTool;

/// Tool to exit plan mode
pub struct ExitPlanModeTool;

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
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::enter_plan_mode(lang))
    }

    fn input_schema(&self) -> serde_json::Value {
        use crate::domain::types::ToolSchema;
        EnterPlanModeInput::data_schema()
    }

    async fn call(
        &self,
        input: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<PlanModeResult> {
        let _args: EnterPlanModeInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(e) => return TypedToolResult::error(serde_json::json!({"status": "error", "message": format!("Invalid input: {}", e), "data": null}).to_string()),
        };

        // Set plan mode in context
        if let Some(mode) = ctx.plan_mode().as_ref() {
            if *mode {
                return TypedToolResult::success("Already in plan mode", PlanModeResult::default());
            }
        }

        // Note: The actual mode change happens in the agent runner
        TypedToolResult::success(
            "Entered plan mode. Tool calls will be simulated and not executed. Use ExitPlanMode to return to normal execution mode.".to_string(),
            PlanModeResult::default(),
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
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::exit_plan_mode(lang))
    }

    fn input_schema(&self) -> serde_json::Value {
        use crate::domain::types::ToolSchema;
        ExitPlanModeInput::data_schema()
    }
    fn data_schema(&self) -> serde_json::Value {
        use crate::domain::types::ToolSchema;
        PlanModeResult::data_schema()
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
        if let Some(mode) = ctx.plan_mode().as_ref() {
            if !*mode {
                return TypedToolResult::success("Not in plan mode", PlanModeResult::default());
            }
        }

        if args.execute.unwrap_or(false) {
            TypedToolResult::success(
                "Exited plan mode. The planned actions will now be executed. Note: Simulated tool calls need to be re-invoked.".to_string(),
                PlanModeResult { reason: String::new(), execute: Some(true) },
            )
        } else {
            TypedToolResult::success(
                "Exited plan mode. Returning to normal execution without running planned actions."
                    .to_string(),
                PlanModeResult {
                    reason: String::new(),
                    execute: Some(false),
                },
            )
        }
    }

    fn is_read_only(&self) -> bool {
        true
    }
}
