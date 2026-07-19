//! tool_coordination — Tool 调用编排：Policy/Hook/审批/并发/结果回收。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 本模块拥有 Main/Sub 共用的调用准备与稳定回收策略。UI 事件、进度流和
//! interaction waiter 仍由各自 adapter 处理；typed continuation 由 #878 收口。

use crate::application::agent::{ToolCall, ToolExecution};
use crate::application::hook_adapter::{RuntimeHookDirective, RuntimeHookReason};
use crate::application::loop_engine::ToolGuardDecision;
use policy::{PolicyDecision, PolicyPort, PolicyRequest};
use std::collections::HashMap;
use std::path::Path;
use tools::{ToolCatalogSnapshot, ToolName};

pub(crate) mod identity;
pub(crate) mod loop_guard;

#[derive(Clone)]
pub(crate) struct DeniedToolCall {
    pub call: ToolCall,
    pub reason: String,
}

pub(crate) struct PreparedToolCall {
    pub call: ToolCall,
    pub authorization: tools::AuthorizationContext,
}

#[derive(Default)]
pub(crate) struct PreparedToolRound {
    pub executable: Vec<PreparedToolCall>,
    pub guard_blocked: Vec<ToolExecution>,
    pub denied: Vec<DeniedToolCall>,
    pub fuse_bypassed: Vec<sdk::ToolCallId>,
}

/// Applies catalog validity, Policy and Runtime guard in canonical order.
///
/// Calls absent from the frozen catalog are denied before Policy because no
/// trustworthy capability set exists. Policy is evaluated once per valid call;
/// its AuthorizationContext decides whether the Runtime fuse remains active.
pub(crate) fn prepare_tool_round(
    calls: &[(ToolCall, ToolGuardDecision)],
    catalog: &ToolCatalogSnapshot,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    workspace_root: &Path,
) -> PreparedToolRound {
    let mut prepared = PreparedToolRound::default();
    for (call, decision) in calls {
        let Some(descriptor) = catalog.find(&ToolName::new(&call.name)) else {
            prepared.denied.push(DeniedToolCall {
                call: call.clone(),
                reason: "Tool is not present in the catalog".to_string(),
            });
            continue;
        };
        let request = match PolicyRequest::new(
            run_id.clone(),
            step_id.clone(),
            ToolName::new(&call.name),
            descriptor.required_capabilities,
            workspace_root,
        ) {
            Ok(request) => request,
            Err(error) => {
                prepared.denied.push(DeniedToolCall {
                    call: call.clone(),
                    reason: error.to_string(),
                });
                continue;
            }
        };
        match policy.evaluate(&request) {
            PolicyDecision::Allow(authorization) => {
                if let ToolGuardDecision::SoftBlock { reason } = decision {
                    if authorization.enforce_tool_fuse {
                        prepared
                            .guard_blocked
                            .push(blocked_tool_execution(call, reason));
                        continue;
                    }
                    prepared.fuse_bypassed.push(call.id.clone());
                }
                prepared.executable.push(PreparedToolCall {
                    call: call.clone(),
                    authorization,
                });
            }
            PolicyDecision::Deny { reason } => prepared.denied.push(DeniedToolCall {
                call: call.clone(),
                reason: format!("{reason:?}"),
            }),
            PolicyDecision::RequireApproval { reason, subject } => {
                prepared.denied.push(DeniedToolCall {
                    call: call.clone(),
                    reason: format!("approval required: {subject:?}: {reason:?}"),
                });
            }
        }
    }
    prepared
}

/// Restores original model call order after concurrent execution and gate paths.
pub(crate) fn restore_tool_call_order(
    calls: &[ToolCall],
    results: Vec<ToolExecution>,
) -> Vec<ToolExecution> {
    let mut by_id: HashMap<_, _> = results
        .into_iter()
        .map(|result| (result.call_id.clone(), result))
        .collect();
    calls
        .iter()
        .filter_map(|call| by_id.remove(&call.id))
        .collect()
}

pub(crate) fn denied_tool_execution(denied: DeniedToolCall) -> ToolExecution {
    ToolExecution::new(&denied.call, tools::ToolOutcome::error(denied.reason))
}

pub(crate) fn blocked_tool_execution(call: &ToolCall, reason: &str) -> ToolExecution {
    let message = format!(
        "Tool call blocked: repeated tool-call loop detected.\n\nReason: {reason}\n\nDo not call this tool again with the same inputs. Use the existing results to summarize findings, change strategy, or ask the user for clarification."
    );
    ToolExecution::new(
        call,
        tools::ToolOutcome {
            text: message.clone(),
            data: serde_json::json!({
                "status": "error",
                "message": message,
                "reason": reason,
                "error_type": "tool_call_loop_fuse",
            }),
            is_error: true,
            images: Vec::new(),
        },
    )
}

// ─── Hook directive application ───────────────────────────────

/// Structured outcome of applying a [`RuntimeHookDirective`] to a single
/// [`ToolCall`].
///
/// The caller is expected to match on the variant to decide the next step
/// (execute, error-synthesize, request approval, or block).
#[derive(Clone)]
pub enum HookDirectiveOutcome {
    /// Tool call is ready to execute with validated, policy-cleared input.
    ///
    /// The `call` carries the **updated** input (from `UpdatedInput` /
    /// `ContextAndInput`). `context` is `Some` only when the directive was
    /// `ContextAndInput`, preserving the hook-injected guidance for the caller.
    Ready {
        /// The call with validated, updated input.
        call: ToolCall,
        /// Context string from `ContextAndInput` (preserved for caller injection).
        context: Option<String>,
        /// Authorization returned by the mandatory post-update Policy evaluation.
        authorization: tools::AuthorizationContext,
    },
    /// Continue with the original call unchanged.
    ///
    /// Produced by `Continue` and `Context` directives. `context` is `Some`
    /// only when the directive was `Context`.
    Continue {
        /// The original, unmodified call.
        call: ToolCall,
        /// Context string from `Context` (preserved for caller injection).
        context: Option<String>,
    },
    /// Updated input failed JSON Schema validation against the frozen catalog
    /// descriptor.
    InvalidInput {
        /// The original call (updated input is discarded).
        call: ToolCall,
        /// Human-readable validation error message.
        error: String,
    },
    /// Policy denied the tool call after re-evaluation with the updated input.
    Denied {
        /// The original call.
        call: ToolCall,
        /// Denial reason.
        reason: String,
    },
    /// Policy requires approval before the updated input may execute.
    ApprovalRequired {
        /// The call with validated, updated input.
        call: ToolCall,
        /// Approval reason.
        reason: String,
    },
    /// Hook explicitly blocked the call.
    Blocked {
        /// The original call.
        call: ToolCall,
        /// Structured block reason from the hook.
        reason: RuntimeHookReason,
    },
}

impl std::fmt::Debug for HookDirectiveOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready {
                call,
                context,
                authorization,
            } => f
                .debug_struct("Ready")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("context", context)
                .field("authorization", authorization)
                .finish(),
            Self::Continue { call, context } => f
                .debug_struct("Continue")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("context", context)
                .finish(),
            Self::InvalidInput { call, error } => f
                .debug_struct("InvalidInput")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("error", error)
                .finish(),
            Self::Denied { call, reason } => f
                .debug_struct("Denied")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("reason", reason)
                .finish(),
            Self::ApprovalRequired { call, reason } => f
                .debug_struct("ApprovalRequired")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("reason", reason)
                .finish(),
            Self::Blocked { call, reason } => f
                .debug_struct("Blocked")
                .field("call_name", &call.name)
                .field("call_index", &call.index)
                .field("reason", reason)
                .finish(),
        }
    }
}

/// Applies a [`RuntimeHookDirective`] to a single [`ToolCall`] and returns a
/// structured [`HookDirectiveOutcome`].
///
/// For directives that update the input (`UpdatedInput` / `ContextAndInput`),
/// the function performs the canonical re-validation sequence:
///
/// 1. Look up the frozen catalog descriptor by tool name.
/// 2. Validate the updated input against the descriptor's `input_schema` via
///    [`tools::validate_tool_input`].
/// 3. Rebuild a [`PolicyRequest`] using the descriptor's `required_capabilities`.
/// 4. Re-evaluate policy.
///
/// Non-mutating directives (`Continue`, `Context`) short-circuit to
/// [`HookDirectiveOutcome::Continue`] without touching the catalog or policy.
/// `Block` maps directly to [`HookDirectiveOutcome::Blocked`].
///
/// This function does **not** call `ToolExecutionPort` — it only decides *what*
/// to do; the caller performs the actual execution.
pub fn apply_hook_directive_to_tool_call(
    call: &ToolCall,
    directive: RuntimeHookDirective,
    catalog: &ToolCatalogSnapshot,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    workspace_root: &Path,
) -> HookDirectiveOutcome {
    match directive {
        RuntimeHookDirective::Continue => HookDirectiveOutcome::Continue {
            call: call.clone(),
            context: None,
        },
        RuntimeHookDirective::Context { context } => HookDirectiveOutcome::Continue {
            call: call.clone(),
            context: Some(context),
        },
        RuntimeHookDirective::Block { reason } => HookDirectiveOutcome::Blocked {
            call: call.clone(),
            reason,
        },
        RuntimeHookDirective::UpdatedInput { input } => revalidate_updated_input(
            call,
            &input,
            None,
            catalog,
            policy,
            run_id,
            step_id,
            workspace_root,
        ),
        RuntimeHookDirective::ContextAndInput { context, input } => revalidate_updated_input(
            call,
            &input,
            Some(context),
            catalog,
            policy,
            run_id,
            step_id,
            workspace_root,
        ),
    }
}

/// Re-validates updated input and re-evaluates policy, returning the
/// appropriate [`HookDirectiveOutcome`].
fn revalidate_updated_input(
    call: &ToolCall,
    input: &serde_json::Value,
    context: Option<String>,
    catalog: &ToolCatalogSnapshot,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    workspace_root: &Path,
) -> HookDirectiveOutcome {
    // 1. Look up the frozen catalog descriptor.
    let Some(descriptor) = catalog.find(&ToolName::new(&call.name)) else {
        return HookDirectiveOutcome::Denied {
            call: call.clone(),
            reason: "Tool is not present in the catalog".to_string(),
        };
    };

    // 2. Validate updated input against the descriptor's JSON Schema.
    if let Err(mismatch) = tools::validate_tool_input(&call.name, &descriptor.input_schema, input) {
        return HookDirectiveOutcome::InvalidInput {
            call: call.clone(),
            error: tools::format_tool_input_error(&mismatch),
        };
    }

    // 3. Rebuild PolicyRequest with the descriptor's required capabilities.
    let request = match PolicyRequest::new(
        run_id.clone(),
        step_id.clone(),
        ToolName::new(&call.name),
        descriptor.required_capabilities,
        workspace_root,
    ) {
        Ok(request) => request,
        Err(error) => {
            return HookDirectiveOutcome::Denied {
                call: call.clone(),
                reason: error.to_string(),
            };
        }
    };

    // 4. Re-evaluate policy against the rebuilt request.
    let updated_call = ToolCall {
        input: input.clone(),
        ..call.clone()
    };
    match policy.evaluate(&request) {
        PolicyDecision::Allow(authorization) => HookDirectiveOutcome::Ready {
            call: updated_call,
            context,
            authorization,
        },
        PolicyDecision::Deny { reason } => HookDirectiveOutcome::Denied {
            call: call.clone(),
            reason: format!("{reason:?}"),
        },
        PolicyDecision::RequireApproval { reason, subject } => {
            HookDirectiveOutcome::ApprovalRequired {
                call: updated_call,
                reason: format!("approval required: {subject:?}: {reason:?}"),
            }
        }
    }
}

#[cfg(test)]
#[path = "tool_coordination_tests.rs"]
mod tests;
