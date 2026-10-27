//! 基线系统提示材质化（Context-owned）。
//!
//! 静态提示、guidance、agent 名册、后台进程段全部由 [`SystemPromptAssembly`]
//! 组装；本模块只负责把它们与 `execution_discipline` 一起排成 block 序列。

use std::sync::Arc;

use async_trait::async_trait;

use crate::adapters::SystemPromptAssembly;
use crate::domain::{ContextRequestData, SystemBlock};
use crate::ports::{ContextPromptSource, PromptMaterialization, PromptMaterializationError};

/// Context-owned baseline prompt materializer：产出恒在的稳定系统块
/// （组装器产物 + execution_discipline）。skill 供给在其上叠加。
pub struct BaselinePromptSource {
    assembly: Arc<SystemPromptAssembly>,
}

impl BaselinePromptSource {
    pub fn new(assembly: Arc<SystemPromptAssembly>) -> Self {
        Self { assembly }
    }

    /// 组装基线 cacheable / uncached 块，供 `SkillPromptSource` 复用。
    pub(crate) async fn baseline_blocks(
        &self,
        request: &ContextRequestData,
    ) -> Result<(Vec<SystemBlock>, Vec<SystemBlock>), PromptMaterializationError> {
        let mut cacheable = self.assembly.materialize(request).await;
        cacheable.push(SystemBlock {
            kind: "execution_discipline".to_string(),
            content: share::i18n::prompt::discipline::universal_execution_discipline(
                request.language.as_str(),
            )
            .to_string(),
            cacheable: true,
            cache_break: false,
        });
        Ok((cacheable, Vec::new()))
    }
}

#[async_trait]
impl ContextPromptSource for BaselinePromptSource {
    async fn materialize(
        &self,
        request: &ContextRequestData,
    ) -> Result<PromptMaterialization, PromptMaterializationError> {
        let (cacheable, uncached) = self.baseline_blocks(request).await?;
        Ok(PromptMaterialization {
            cacheable,
            uncached,
        })
    }
}
