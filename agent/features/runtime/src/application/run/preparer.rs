use std::sync::Arc;

use crate::application::run::context_factory::RuntimeContextFactory;
use crate::application::run::preparation::{
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

    pub fn prepare(
        &self,
        request: RunPreparationRequest,
    ) -> Result<PreparedRun, RunPreparationError> {
        let (context, session, workspace) = self.context_factory.prepare(&request)?;
        let request = request.with_session(session);
        let spec = request.spec().clone();
        let session = request.session().clone();
        let parent_run_id = request.parent().map(|parent| parent.run_id().clone());
        Ok(PreparedRun::with_context(
            spec,
            parent_run_id,
            session,
            context,
            workspace,
        ))
    }
}
