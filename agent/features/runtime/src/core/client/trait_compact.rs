use sdk::SdkError;

use super::accessors::AgentClientImpl;

type Result<T> = std::result::Result<T, SdkError>;

pub(super) async fn compact_messages_impl(
    me: &AgentClientImpl,
    messages: Vec<sdk::ChatMessage>,
    system_prompt: &str,
    context_size: usize,
) -> Result<(Vec<sdk::ChatMessage>, bool)> {
    let runtime_messages: Vec<share::message::Message> = messages
        .into_iter()
        .map(super::mapping::message_from_sdk)
        .collect();
    let client = me
        .inner
        .current_client
        .read()
        .ok()
        .map(|guard| (*guard).clone());
    let result = crate::business::compact::compact_messages_with_llm(
        &runtime_messages,
        system_prompt,
        context_size,
        client.as_ref().map(|c| c.as_ref()),
        None,
    )
    .await;
    match result {
        Some(cr) => {
            let sdk_messages: Vec<sdk::ChatMessage> = cr
                .recent_messages
                .into_iter()
                .map(super::mapping::message_to_sdk)
                .collect();
            Ok((sdk_messages, true))
        }
        None => {
            let sdk_messages: Vec<sdk::ChatMessage> = runtime_messages
                .into_iter()
                .map(super::mapping::message_to_sdk)
                .collect();
            Ok((sdk_messages, false))
        }
    }
}

pub(super) async fn compact_impl(_me: &AgentClientImpl) -> Result<()> {
    Ok(())
}

pub(super) async fn estimate_context_impl(
    me: &AgentClientImpl,
    messages: &[sdk::ChatMessage],
    system_prompt: &str,
) -> Result<sdk::ContextEstimate> {
    let runtime_messages: Vec<share::message::Message> = messages
        .iter()
        .map(|msg| super::mapping::message_from_sdk(msg.clone()))
        .collect();
    let estimated = crate::business::compact::estimate_messages_tokens(&runtime_messages)
        + crate::business::compact::estimate_tokens(system_prompt);
    let context_size = me.inner.context.resources.context_size;
    let pct = if context_size > 0 {
        estimated as f64 * 100.0 / context_size as f64
    } else {
        0.0
    };
    Ok(sdk::ContextEstimate {
        estimated_tokens: estimated,
        system_tokens: crate::business::compact::estimate_tokens(system_prompt),
        context_size,
        usage_percentage: pct,
    })
}
