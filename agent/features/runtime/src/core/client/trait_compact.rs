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
    let (compacted, was_compacted) = crate::business::compact::compact_messages_with_llm(
        &runtime_messages,
        system_prompt,
        context_size,
        client.as_ref().map(|c| c.as_ref()),
    )
    .await;
    let sdk_messages: Vec<sdk::ChatMessage> = compacted
        .into_iter()
        .map(super::mapping::message_to_sdk)
        .collect();
    Ok((sdk_messages, was_compacted))
}

pub(super) async fn compact_impl(_me: &AgentClientImpl) -> Result<()> {
    Ok(())
}
