use futures::stream;
use provider::{
    InvocationDelta, InvocationEvent, InvocationStream, ProviderCompletion, ProviderContentBlock,
    ProviderStopReason, RawUsageSnapshot, ReasoningLevel,
};

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
