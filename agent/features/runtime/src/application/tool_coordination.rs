//! tool_coordination — Tool 调用编排：Policy/Hook/审批/并发/结果回收。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 本模块拥有 Main/Sub 共用的调用准备与稳定回收策略。UI 事件、进度流和
//! interaction waiter 仍由各自 adapter 处理；typed continuation 由 #878 收口。

use crate::application::agent::{ToolCall, ToolExecution};
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

#[cfg(test)]
#[path = "tool_coordination_tests.rs"]
mod tests;
