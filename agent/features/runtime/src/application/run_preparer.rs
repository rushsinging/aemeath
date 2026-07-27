use std::sync::Arc;

use crate::application::runtime_context::{RunCapabilityBindings, RuntimeContext};
use crate::application::runtime_context_factory::RuntimeContextFactory;
use crate::application::runtime_preparation::{
    PreparedRun, RunPreparationError, RunPreparationRequest,
};

/// Run preparation use case. It coordinates domain Run creation with the
/// context factory; RuntimeContextFactory itself only creates RuntimeContext.
pub struct RunPreparer {
    context_factory: Arc<RuntimeContextFactory>,
}

impl RunPreparer {
    pub fn new(context_factory: Arc<RuntimeContextFactory>) -> Self {
        Self { context_factory }
    }

    pub fn context_factory(&self) -> &RuntimeContextFactory {
        self.context_factory.as_ref()
    }

    pub fn prepare(
        &self,
        request: RunPreparationRequest,
        bindings: RunCapabilityBindings,
        parent: Option<&RuntimeContext>,
    ) -> Result<PreparedRun, RunPreparationError> {
        let spec = request.spec().clone();
        let session = request.session().clone();
        let parent_run_id = request.parent().map(|parent| parent.run_id().clone());
        let context = self
            .context_factory
            .assemble(&spec, bindings, parent)
            .map_err(|_| RunPreparationError::ContextAssembly)?;
        Ok(PreparedRun::with_context(
            spec,
            parent_run_id,
            session,
            context,
        ))
    }
}
