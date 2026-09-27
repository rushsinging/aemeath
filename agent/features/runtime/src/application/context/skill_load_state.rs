//! Context-owned Skill 加载状态到 Tools Published Language 的窄 ACL adapter。

use std::sync::Arc;

use async_trait::async_trait;

use crate::ports::ContextPort;

pub(crate) struct ContextSkillLoadState {
    context: Arc<dyn ContextPort>,
}

impl ContextSkillLoadState {
    pub(crate) fn new(context: Arc<dyn ContextPort>) -> Self {
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
