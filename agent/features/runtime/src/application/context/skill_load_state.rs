//! Context-owned Skill 加载状态到 Tools Published Language 的窄 ACL adapter。

use std::sync::Arc;

use async_trait::async_trait;

use context::ContextAppendPort;

pub(crate) struct ContextSkillLoadState {
    context: Arc<dyn ContextAppendPort>,
}

impl ContextSkillLoadState {
    pub(crate) fn new(context: Arc<dyn ContextAppendPort>) -> Self {
        Self { context }
    }
}

#[async_trait]
impl tools::published::skill::SkillLoadStatePort for ContextSkillLoadState {
    async fn compare_and_record(
        &self,
        mutation: tools::published::skill::SkillLoadMutation,
    ) -> Result<
        tools::published::skill::SkillLoadDecision,
        tools::published::skill::SkillLoadStateError,
    > {
        self.context.compare_and_record_skill_load(mutation).await
    }
}
