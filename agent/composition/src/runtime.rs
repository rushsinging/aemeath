pub type AgentArgs = sdk::ChatBootstrapArgs;

use crate::app::FeatureGateways;

pub(crate) use runtime::api::AgentClientImpl;

pub(crate) async fn from_args_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
) -> Result<AgentClientImpl, sdk::SdkError> {
    let _ = (
        gateways.tools.new_registry(),
        std::sync::Arc::strong_count(&gateways.provider),
        std::sync::Arc::strong_count(&gateways.project),
    );

    runtime::api::from_args(args).await
}
