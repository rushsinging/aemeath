use async_trait::async_trait;

use crate::domain::{ContextRequest, SystemBlock};
use crate::ports::{ContextPromptSource, PromptMaterialization, PromptMaterializationError};

/// Context-owned baseline prompt materializer. Produces the stable, always-present
/// system blocks (system_prompt + execution_discipline). Rich guidance/skill
/// suppliers compose around this baseline.
pub struct BaselinePromptSource;

impl BaselinePromptSource {
    /// 组装基线 cacheable / uncached 块，供 `SkillPromptSource` 复用以避免逻辑重复。
    pub(crate) fn baseline_blocks(
        request: &ContextRequest,
    ) -> (Vec<SystemBlock>, Vec<SystemBlock>) {
        let cacheable = vec![
            SystemBlock {
                kind: "system_prompt".to_string(),
                content: request.system_prompt.as_str().to_string(),
                cacheable: true,
                cache_break: false,
            },
            SystemBlock {
                kind: "execution_discipline".to_string(),
                content: crate::adapters::prompt::universal_execution_discipline(
                    request.language.as_str(),
                )
                .to_string(),
                cacheable: true,
                cache_break: false,
            },
        ];
        (cacheable, Vec::new())
    }
}

#[async_trait]
impl ContextPromptSource for BaselinePromptSource {
    async fn materialize(
        &self,
        request: &ContextRequest,
    ) -> Result<PromptMaterialization, PromptMaterializationError> {
        let (cacheable, uncached) = Self::baseline_blocks(request);
        Ok(PromptMaterialization {
            cacheable,
            uncached,
            revision: 0,
        })
    }
}
