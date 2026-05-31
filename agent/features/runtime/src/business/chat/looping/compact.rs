use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent};
use hook::api::{CompactHookData, HookData, HookRunner};
use share::config::hooks::HookEvent;
use share::message::Message;
use std::sync::Arc;

/// Run auto-compaction if the context is approaching the limit.
/// Returns true if the messages were modified.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn auto_compact<S>(
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &HookRunner,
    turn_count: usize,
    messages: &mut Vec<Message>,
    system_prompt_text: &str,
    context_size: usize,
    tool_schema_tokens: usize,
    last_api_input_tokens: u64,
    llm_client: &Arc<provider::api::LlmClient>,
) -> bool
where
    S: ChatEventSink,
{
    use crate::business::compact;

    // PreCompact hook
    let pre_compact_results = hook_ui
        .run_json(
            hook_runner,
            HookEvent::PreCompact,
            None,
            HookData::Compact(CompactHookData {
                turns: turn_count,
                messages_before: messages.len(),
                messages_after: None,
                was_compacted: false,
            }),
        )
        .await;
    let pre_compact_blocked = pre_compact_results.iter().any(|(_, result, json)| {
        result.blocked
            || json
                .as_ref()
                .is_some_and(|j| j.decision.as_deref() == Some("block"))
    });
    for (_entry, _result, json_output) in &pre_compact_results {
        if let Some(json) = json_output {
            if let Some(ref ctx) = json.additional_context {
                let _ = sink
                    .send_event(RuntimeStreamEvent::SystemMessage(ctx.clone()))
                    .await;
            }
            if let Some(ref msg) = json.system_message {
                let _ = sink
                    .send_event(RuntimeStreamEvent::SystemMessage(msg.clone()))
                    .await;
            }
        }
    }

    if pre_compact_blocked {
        log::warn!("PreCompact hook blocked compaction");
        return false;
    }

    let should_compact = if last_api_input_tokens > 0 {
        compact::needs_compaction_actual(last_api_input_tokens, 0, context_size)
    } else {
        compact::needs_compaction_full(
            messages,
            system_prompt_text,
            context_size,
            tool_schema_tokens,
        )
    };

    if !should_compact || messages.len() <= 4 {
        return false;
    }

    let old_len = messages.len();
    compact::microcompact(messages, 10);
    if compact::needs_compaction_full(
        messages,
        system_prompt_text,
        context_size,
        tool_schema_tokens,
    ) || (last_api_input_tokens > 0
        && compact::needs_compaction_actual(last_api_input_tokens, 0, context_size))
    {
        let (compacted, was_compacted) = compact::compact_messages_with_llm(
            messages,
            system_prompt_text,
            context_size,
            Some(llm_client.as_ref()),
        )
        .await;
        if was_compacted {
            let new_len = compacted.len();
            *messages = compacted;
            let _ = sink
                .send_event(RuntimeStreamEvent::SystemMessage(format!(
                    "[auto-compacted: {} → {} messages]",
                    old_len, new_len
                )))
                .await;

            // PostCompact hook
            let post_compact_results = hook_ui
                .run_json(
                    hook_runner,
                    HookEvent::PostCompact,
                    None,
                    HookData::Compact(CompactHookData {
                        turns: turn_count,
                        messages_before: old_len,
                        messages_after: Some(new_len),
                        was_compacted: true,
                    }),
                )
                .await;
            for (_entry, _result, json_output) in &post_compact_results {
                if let Some(json) = json_output {
                    if let Some(ref ctx) = json.additional_context {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::SystemMessage(ctx.clone()))
                            .await;
                    }
                    if let Some(ref msg) = json.system_message {
                        let _ = sink
                            .send_event(RuntimeStreamEvent::SystemMessage(msg.clone()))
                            .await;
                    }
                }
            }
            return true;
        }
    }
    false
}
