pub type AgentArgs = sdk::ChatBootstrapArgs;

use crate::app::FeatureGateways;

pub(crate) use runtime::AgentClientImpl;

pub(crate) async fn from_args_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
    workspace: project::WorkspaceViews,
    config: config::ConfigWiring,
) -> Result<AgentClientImpl, sdk::SdkError> {
    runtime::from_args_with_workspace(
        args,
        workspace,
        config.reader(),
        config.query(),
        config.writer(),
        gateways.provider,
        gateways.tools,
    )
    .await
}
