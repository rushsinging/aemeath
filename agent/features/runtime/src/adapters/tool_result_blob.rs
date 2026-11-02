use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use storage::{
    AtomicBlobPort, DurabilityData, GenerationData, ReadOutcomeData, SafePathSegmentData,
    StorageKeyData, StorageNamespaceData, WriteOptionsData,
};

use crate::ports::{ToolResultBlobError, ToolResultBlobPort, ToolResultBlobRef};

pub struct AtomicBlobToolResultStore {
    blob: Arc<dyn AtomicBlobPort>,
    locator_root: PathBuf,
}

impl AtomicBlobToolResultStore {
    pub fn new(blob: Arc<dyn AtomicBlobPort>, locator_root: PathBuf) -> Self {
        Self { blob, locator_root }
    }

    fn key(
        session_id: &str,
        tool_use_id: &str,
    ) -> Result<(StorageKeyData, SafePathSegmentData, SafePathSegmentData), ToolResultBlobError>
    {
        let session = SafePathSegmentData::from_str(session_id)
            .map_err(|error| ToolResultBlobError::invalid_key(error.to_string()))?;
        let tool = SafePathSegmentData::from_str(tool_use_id)
            .map_err(|error| ToolResultBlobError::invalid_key(error.to_string()))?;
        let key = StorageKeyData::new(
            StorageNamespaceData::ToolResult,
            vec![session.clone(), tool.clone()],
        )
        .map_err(|error| ToolResultBlobError::invalid_key(error.to_string()))?;
        Ok((key, session, tool))
    }

    fn locator(
        &self,
        session: &SafePathSegmentData,
        tool: &SafePathSegmentData,
    ) -> ToolResultBlobRef {
        // #1865 D：物理布局单点在 storage（StorageKeyData::relative_path），
        // 本处只组装逻辑 key，不再复制 namespace/segment 顺序知识。
        let key = StorageKeyData::new(
            StorageNamespaceData::ToolResult,
            vec![session.clone(), tool.clone()],
        )
        .expect("session/tool 段已在 validate_key 校验");
        ToolResultBlobRef::new(
            self.locator_root
                .join(key.relative_path())
                .display()
                .to_string(),
        )
    }
}

#[async_trait]
impl ToolResultBlobPort for AtomicBlobToolResultStore {
    async fn write_once(
        &self,
        session_id: &str,
        tool_use_id: &str,
        bytes: &[u8],
    ) -> Result<ToolResultBlobRef, ToolResultBlobError> {
        let (key, session, tool) = Self::key(session_id, tool_use_id)?;
        match self
            .blob
            .read(&key, GenerationData::Primary)
            .await
            .map_err(|error| ToolResultBlobError::write(error.to_string()))?
        {
            ReadOutcomeData::Found(existing) if existing.bytes() == bytes => {
                return Ok(self.locator(&session, &tool));
            }
            ReadOutcomeData::Found(_) => {
                return Err(ToolResultBlobError::conflict("工具结果标识已对应不同内容"));
            }
            ReadOutcomeData::NotFound => {}
        }
        self.blob
            .write_atomic(
                &key,
                bytes,
                WriteOptionsData::new(DurabilityData::ProcessCrashSafe),
            )
            .await
            .map_err(|error| ToolResultBlobError::write(error.to_string()))?;
        Ok(self.locator(&session, &tool))
    }
}
