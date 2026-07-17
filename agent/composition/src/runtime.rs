pub type AgentArgs = sdk::ChatBootstrapArgs;

use crate::app::FeatureGateways;

pub(crate) use runtime::AgentClientImpl;

pub(crate) async fn from_args_with_gateways(
    args: AgentArgs,
    _gateways: FeatureGateways,
    workspace: project::WorkspaceViews,
    config: config::ConfigWiring,
) -> Result<AgentClientImpl, sdk::SdkError> {
    // TODO(#47): This composition-level wiring scaffold will consume gateways
    // when the runtime bootstrap migration is ready for feature gateway injection.
    runtime::from_args_with_workspace(
        args,
        workspace,
        config.reader(),
        config.query(),
        config.writer(),
    )
    .await
}
