use std::sync::Arc;

use async_trait::async_trait;

pub(crate) fn test_tool_result_materializer(
) -> Arc<super::tool_result_materializer::ToolResultMaterializer> {
    struct TestBlobPort;

    #[async_trait]
    impl crate::ports::ToolResultBlobPort for TestBlobPort {
        async fn write_once(
            &self,
            session_id: &str,
            tool_use_id: &str,
            _bytes: &[u8],
        ) -> Result<crate::ports::ToolResultBlobRef, crate::ports::ToolResultBlobError> {
            Ok(crate::ports::ToolResultBlobRef::new(format!(
                "tool-result://{session_id}/{tool_use_id}"
            )))
        }
    }

    Arc::new(
        super::tool_result_materializer::ToolResultMaterializer::new(
            Arc::new(TestBlobPort),
            super::tool_result_materializer::ToolResultMaterializationPolicy::new(
                50_000, 2_000, 500,
            ),
        ),
    )
}
