use async_trait::async_trait;

use crate::domain::{ContextRequest, SystemBlock};
use crate::ports::{ContextMemorySource, MemoryMaterialization};

/// Sub Run 或禁用 Memory 时使用的空注入 adapter。
pub struct NoOpContextMemorySource;

#[async_trait]
impl ContextMemorySource for NoOpContextMemorySource {
    async fn materialize(
        &self,
        _request: &ContextRequest,
    ) -> Result<MemoryMaterialization, String> {
        Ok(MemoryMaterialization {
            blocks: Vec::<SystemBlock>::new(),
            revision: 0,
        })
    }
}
