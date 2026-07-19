use async_trait::async_trait;

use crate::domain::{ContextRequest, SystemBlock};
use crate::ports::{ContextPromptSource, PromptMaterialization};

/// Context-owned baseline prompt materializer. Rich guidance/skill suppliers can
/// replace this adapter without changing Runtime's ContextPort contract.
pub struct BaselinePromptSource;

#[async_trait]
impl ContextPromptSource for BaselinePromptSource {
    async fn materialize(&self, request: &ContextRequest) -> Result<PromptMaterialization, String> {
        let mut cacheable = vec![SystemBlock {
            kind: "system_prompt".to_string(),
            content: request.system_prompt.as_str().to_string(),
            cacheable: true,
        }];
        let language = request.language.as_str();
        cacheable.push(SystemBlock {
            kind: "execution_discipline".to_string(),
            content: crate::adapters::prompt::universal_execution_discipline(language).to_string(),
            cacheable: true,
        });
        Ok(PromptMaterialization {
            cacheable,
            uncached: vec![SystemBlock {
                kind: "current_date".to_string(),
                content: request.current_date.as_str().to_string(),
                cacheable: false,
            }],
            revision: 0,
        })
    }
}
