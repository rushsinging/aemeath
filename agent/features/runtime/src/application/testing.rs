use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use provider::{
    InvocationDelta, InvocationEvent, InvocationStream, ProviderCompletion, ProviderContentBlock,
    ProviderStopReason, RawUsageSnapshot, ReasoningLevel,
};

pub(crate) fn test_tool_result_materializer(
) -> Arc<crate::application::tool_result_materialization::ToolResultMaterializer> {
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
        crate::application::tool_result_materialization::ToolResultMaterializer::new(
            Arc::new(TestBlobPort),
            crate::application::tool_result_materialization::ToolResultMaterializationPolicy::new(
                50_000, 2_000, 500,
            ),
        ),
    )
}

pub(crate) fn text_completion_stream(
    text: impl Into<String>,
    input_tokens: u32,
    output_tokens: u32,
) -> InvocationStream {
    let text = text.into();
    Box::pin(stream::iter([
        InvocationEvent::Delta(InvocationDelta::Text(text.clone())),
        InvocationEvent::Completed(ProviderCompletion {
            output: vec![ProviderContentBlock::Text(text)],
            stop_reason: ProviderStopReason::EndTurn,
            usage: Some(RawUsageSnapshot {
                input_tokens: Some(input_tokens),
                output_tokens: Some(output_tokens),
                ..RawUsageSnapshot::default()
            }),
            effective_reasoning: ReasoningLevel::Off,
        }),
    ]))
}
