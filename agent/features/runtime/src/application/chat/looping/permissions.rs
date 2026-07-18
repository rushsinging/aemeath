use crate::application::agent::ToolCall;
use policy::{PolicyDecision, PolicyPort, PolicyRequest};
use tools::{ToolCatalogSnapshot, ToolName};

use super::engine::DeniedCall;

pub(crate) fn evaluate_calls(
    tool_calls: &[ToolCall],
    catalog: &ToolCatalogSnapshot,
    policy: &dyn PolicyPort,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    workspace_root: &std::path::Path,
) -> (Vec<ToolCall>, Vec<DeniedCall>) {
    let mut approved = Vec::with_capacity(tool_calls.len());
    let mut denied = Vec::new();
    for call in tool_calls {
        let Some(descriptor) = catalog.find(&ToolName::new(&call.name)) else {
            denied.push(denied_call(call, "Tool is not present in the catalog"));
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
                denied.push(denied_call(call, &error.to_string()));
                continue;
            }
        };
        match policy.evaluate(&request) {
            PolicyDecision::Allow => approved.push(call.clone()),
            PolicyDecision::Deny { reason } => {
                denied.push(denied_call(call, &format!("{reason:?}")))
            }
            PolicyDecision::RequireApproval { reason, subject } => denied.push(denied_call(
                call,
                &format!("approval required: {subject:?}: {reason:?}"),
            )),
        }
    }
    (approved, denied)
}

fn denied_call(call: &ToolCall, reason: &str) -> DeniedCall {
    DeniedCall {
        id: call.id.to_string(),
        provider_id: call.provider_id.clone(),
        name: call.name.clone(),
        reason: reason.to_string(),
    }
}
