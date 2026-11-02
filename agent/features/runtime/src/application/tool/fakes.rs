//! tool 层测试替身（crate 内 lib 测试专用；跨 crate 共享的 runtime fake 在
//! `testkit::runtime` 维护——本模块不对外导出）。

use std::sync::Arc;

use async_trait::async_trait;

use crate::ports::{ToolResultBlobError, ToolResultBlobPort, ToolResultBlobRef};

pub(crate) fn test_tool_result_materializer(
) -> Arc<super::tool_result_materializer::ToolResultMaterializer> {
    struct TestBlobPort;

    #[async_trait]
    impl ToolResultBlobPort for TestBlobPort {
        async fn write_once(
            &self,
            session_id: &str,
            tool_use_id: &str,
            _bytes: &[u8],
        ) -> Result<ToolResultBlobRef, ToolResultBlobError> {
            Ok(ToolResultBlobRef::new(format!(
                "tool-result://{session_id}/{tool_use_id}"
            )))
        }
    }

    Arc::new(
        super::tool_result_materializer::ToolResultMaterializer::new(
            Arc::new(TestBlobPort),
            super::tool_result_materializer::ToolResultMaterializationPolicyData::new(
                50_000, 2_000, 500,
            ),
        ),
    )
}
