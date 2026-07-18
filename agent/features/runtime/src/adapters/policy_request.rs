use policy::PolicyRequest;
use sdk::{RunId, RunStepId};
#[cfg(test)]
use tools::ToolCapability;
use tools::{ToolCapabilities, ToolName};

pub(crate) fn adapt_policy_request(
    run_id: &RunId,
    step_id: &RunStepId,
    tool_name: &str,
    required_capabilities: ToolCapabilities,
    workspace_root: &std::path::Path,
) -> Result<PolicyRequest, policy::PolicyRequestError> {
    PolicyRequest::new(
        run_id.clone(),
        step_id.clone(),
        ToolName::new(tool_name),
        required_capabilities,
        workspace_root,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_adapter_preserves_all_runtime_facts() {
        let run_id = RunId::new_v7();
        let step_id = RunStepId::new_v7();
        let caps = ToolCapabilities::single(ToolCapability::WriteWorkspace);
        let request = adapt_policy_request(
            &run_id,
            &step_id,
            "Edit",
            caps,
            std::path::Path::new("/workspace"),
        )
        .unwrap();

        assert_eq!(request.run_id(), &run_id);
        assert_eq!(request.run_step_id(), &step_id);
        assert_eq!(request.tool_name(), &ToolName::new("Edit"));
        assert_eq!(request.required_capabilities(), caps);
        assert_eq!(request.workspace_root(), std::path::Path::new("/workspace"));
    }
}
