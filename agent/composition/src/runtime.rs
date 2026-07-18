pub type AgentArgs = sdk::ChatBootstrapArgs;

use crate::app::FeatureGateways;

pub(crate) use runtime::AgentClientImpl;

pub(crate) async fn from_args_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
    workspace: project::WorkspaceViews,
    config: config::ConfigWiring,
) -> Result<AgentClientImpl, sdk::SdkError> {
    // Task BC wiring: Composition owns the single backing and its persistence envelope.
    let task_wiring = task::wire_task();
    let task_access = task_wiring.access();
    let session_tasks = context::compose_session_task_capture(task_wiring.persist());

    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        workspace,
        runtime::RuntimeConfigDependencies::new(config.reader(), config.query(), config.writer()),
        gateways.provider,
        gateways.tools,
        task_access,
        session_tasks,
    );
    runtime::from_args_with_workspace(args, dependencies).await
}
