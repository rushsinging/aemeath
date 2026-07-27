use std::sync::Arc;

use crate::application::run::context_factory::{RuntimeContextFactory, RuntimeContextResolver};
use crate::application::run::preparation::{
    PreparedRun, RunPreparationError, RunPreparationRequest,
};

/// Run preparation use case. It coordinates domain Run creation with the
/// context factory; RuntimeContextFactory itself only creates RuntimeContext.
pub struct RunPreparer {
    context_factory: Arc<RuntimeContextFactory>,
    context_resolver: Arc<dyn RuntimeContextResolver>,
}

impl RunPreparer {
    pub fn new(
        context_factory: Arc<RuntimeContextFactory>,
        context_resolver: Arc<dyn RuntimeContextResolver>,
    ) -> Self {
        Self {
            context_factory,
            context_resolver,
        }
    }

    pub fn prepare(
        &self,
        request: RunPreparationRequest,
    ) -> Result<PreparedRun, RunPreparationError> {
        let (context, session) = self
            .context_resolver
            .resolve(self.context_factory.as_ref(), &request)?;
        let request = request.with_session(session);
        let spec = request.spec().clone();
        let session = request.session().clone();
        let parent_run_id = request.parent().map(|parent| parent.run_id().clone());
        Ok(PreparedRun::with_context(
            spec,
            parent_run_id,
            session,
            context,
        ))
    }
}
