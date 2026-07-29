use std::sync::Arc;

use crate::application::run::context_factory::RuntimeContextFactory;
use crate::application::run::creation::{
    RunCreationBindings, RunCreationError, RunCreationRequest, RunInstance,
};

/// 完整 Run 实例的唯一创建入口。
///
/// `RuntimeContextFactory` 只创建 `RuntimeContext`；本 Factory 负责协调领域
/// `Run`、执行状态、Session 快照与绑定后的 Context，使其作为一个实例返回。
pub struct RunFactory {
    context_factory: Arc<RuntimeContextFactory>,
    bindings: RunCreationBindings,
}

impl RunFactory {
    pub(crate) fn for_session(
        context_factory: Arc<RuntimeContextFactory>,
        bindings: crate::application::run::creation::SessionRunBindings,
    ) -> Self {
        Self {
            context_factory,
            bindings: RunCreationBindings::Session(bindings),
        }
    }

    pub(crate) fn for_parent(
        context_factory: Arc<RuntimeContextFactory>,
        bindings: crate::application::run::creation::ParentRunBindings,
    ) -> Self {
        Self {
            context_factory,
            bindings: RunCreationBindings::Parent(bindings),
        }
    }

    pub(crate) fn create(
        &self,
        request: RunCreationRequest,
    ) -> Result<RunInstance, RunCreationError> {
        let (context, session, workspace) =
            self.context_factory.prepare(&request, &self.bindings)?;
        let request = request.with_session(session);
        let spec = request.spec().clone();
        let session = request.session().clone();
        let parent_run_id = request.parent().map(|parent| parent.run_id().clone());
        Ok(RunInstance::new(
            spec,
            parent_run_id,
            session,
            context,
            workspace,
        ))
    }
}
