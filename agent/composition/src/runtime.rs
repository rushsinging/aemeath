pub type AgentArgs = sdk::ChatBootstrapArgs;

use crate::app::FeatureGateways;

pub(crate) use runtime::AgentClientImpl;

pub(crate) async fn from_args_with_gateways(
    args: AgentArgs,
    _gateways: FeatureGateways,
    workspace: project::WorkspaceViews,
) -> Result<AgentClientImpl, sdk::SdkError> {
    // Task BC wiring: Composition owns the single backing and its persistence envelope.
    // task_access → Runtime/Tools daily state (registry, reminder, status, finalize).
    // session_tasks → Context-owned capture-only facade (no restore authority leaks to Runtime).
    let task_wiring = task::wire_task();
    let task_access = task_wiring.access();
    let session_tasks = context::compose_session_task_capture(task_wiring.persist());

    runtime::from_args_with_workspace(args, workspace, task_access, session_tasks).await
}
