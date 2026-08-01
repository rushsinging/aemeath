use std::sync::Arc;

use crate::application::run::context_factory::RuntimeContextFactory;
use crate::application::run::creation::{
    ParentRunBindings, ParentRunFacts, RunCreationError, RunCreationRequest, RunInstance,
};
use crate::application::run::factory::RunFactory;
use crate::application::run::workspace::RuntimeWorkspaceAccess;
use crate::domain::agent_run::{RunId, RunSpec};
use crate::ports::ProviderFactory;

use super::doubles::{FakeProviderFactory, FakeSkillCatalog};

pub(crate) struct ParentRunFixture {
    context_factory: Arc<RuntimeContextFactory>,
    provider_factory: Arc<dyn ProviderFactory>,
    skill_catalog: Arc<dyn tools::SkillCatalogPort>,
}

impl ParentRunFixture {
    pub(crate) fn new(context_factory: Arc<RuntimeContextFactory>) -> Self {
        Self {
            context_factory,
            provider_factory: Arc::new(FakeProviderFactory),
            skill_catalog: Arc::new(FakeSkillCatalog),
        }
    }

    pub(crate) fn create(
        &self,
        spec: RunSpec,
        session: crate::application::run::creation::SessionSnapshot,
        parent_run_id: RunId,
        parent_spec: RunSpec,
        parent_context: Arc<crate::application::run::context::RuntimeContext>,
        parent_workspace: RuntimeWorkspaceAccess,
    ) -> Result<RunInstance, RunCreationError> {
        let request = RunCreationRequest::new(
            spec,
            session,
            Some(ParentRunFacts::new(parent_run_id, parent_spec)),
        )?;
        let bindings = ParentRunBindings::from_active_run(parent_context, parent_workspace);
        RunFactory::for_parent(
            Arc::new(
                self.context_factory.with_derived_bindings(
                    self.provider_factory.clone(),
                    self.skill_catalog.clone(),
                ),
            ),
            bindings,
        )
        .create(request)
    }
}
