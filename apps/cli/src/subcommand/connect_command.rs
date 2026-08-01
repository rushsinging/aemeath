use std::io::{self, IsTerminal};
use std::sync::Arc;

pub(crate) async fn run_connect_command_with_origin(
    forms: Arc<dyn sdk::ConfigFormClient>,
    origin: sdk::ConfigFormOrigin,
) -> Result<sdk::ConfigFormTerminal, sdk::SdkError> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(sdk::SdkError::Init(
            "aemeath connect 需要交互终端".to_string(),
        ));
    }
    super::config_form_command::run_config_form(
        forms,
        sdk::ConfigFormWorkflowId("provider_connect".to_string()),
        origin,
    )
    .await
}

pub(crate) async fn run_connect_command(
    forms: Arc<dyn sdk::ConfigFormClient>,
) -> Result<(), sdk::SdkError> {
    run_connect_command_with_origin(forms, sdk::ConfigFormOrigin::ExplicitCommand)
        .await
        .map(|_| ())
}
