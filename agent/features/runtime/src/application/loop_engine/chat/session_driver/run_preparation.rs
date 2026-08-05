use std::sync::Arc;

use crate::application::run::creation::{
    RunCreationError, RunCreationRequest, RunInstance, SessionRunBindings,
};
use crate::application::run::factory::RunFactory;
use crate::domain::agent_run::RunSpec;

pub(super) struct MainRunPreparation {
    pub run_config: crate::application::run::config::RunConfigSnapshot,
    pub request: RunCreationRequest,
    pub session_bindings: SessionRunBindings,
}

pub(super) fn prepare_main_run(
    shell: &crate::application::client::SessionRuntime,
    wiring: &Arc<context::MainSessionWiring>,
    reasoning: &Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
    sink_handle: &crate::application::loop_engine::chat::ChatEventSinkHandle,
    session_usage: &crate::application::run::context::RunUsageTracker,
) -> Result<MainRunPreparation, RunCreationError> {
    let run_config =
        crate::application::run::config::RunConfigSnapshot::capture(wiring.committed_config());
    let session_snapshot = {
        let binding = shell.model_state.binding();
        let mut session_state = shell
            .session_state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        session_state.update_provider_binding(binding.as_ref(), wiring.committed_config());
        session_state.snapshot_for_run()
    };
    let session_bindings = SessionRunBindings::new(
        wiring.clone(),
        shell.model_state.binding(),
        shell.interaction_bridge.clone(),
        reasoning.clone(),
        sink_handle.clone(),
        session_usage.clone(),
    );
    let request = RunCreationRequest::new(RunSpec::main(), session_snapshot, None)?;
    Ok(MainRunPreparation {
        run_config,
        request,
        session_bindings,
    })
}

pub(super) fn create_main_run(
    shell: &crate::application::client::SessionRuntime,
    preparation: MainRunPreparation,
) -> Result<RunInstance, RunCreationError> {
    let run_factory = RunFactory::for_session(
        shell.runtime_context_factory.clone(),
        preparation.session_bindings,
    );
    run_factory.create(preparation.request)
}
