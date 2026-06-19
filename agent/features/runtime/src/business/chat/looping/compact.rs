use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent};
use crate::LOG_TARGET;
use hook::api::{CompactHookData, HookData, HookRunner};
use share::config::hooks::HookEvent;
use share::message::Message;
use std::sync::Arc;

/// compact 结果：summary 走 system 通道，messages 为 recent tail。
pub(crate) struct CompactOutcome {
    /// 早期对话摘要（调用方拼入 system_blocks）
    pub summary: String,
    /// recent tail（替换活跃链的 messages）
    pub messages: Vec<Message>,
}

/// Run auto-compaction if the context is approaching the limit.
///
/// 返回 `Some(CompactOutcome)` 表示发生了压缩（summary + recent tail）。
/// 返回 `None` 表示无需压缩。
///
/// resume 保护：首 turn 无 API 反馈时跳过（`turn_count == 1 && last_api_input_tokens == 0`），
/// 确保 resume 会话不会在第一轮被误判 compact。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn auto_compact<S>(
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &HookRunner,
    turn_count: usize,
    messages: &[Message],
    system_prompt_text: &str,
    context_size: usize,
    tool_schema_tokens: usize,
    last_api_input_tokens: u64,
    last_api_output_tokens: u64,
    cached_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    memory_config: &share::config::MemoryConfig,
    cwd: &std::path::Path,
    llm_client: &Arc<provider::api::LlmClient>,
) -> Option<CompactOutcome>
where
    S: ChatEventSink,
{
    use crate::business::compact;

    // resume 保护：首 turn 无 API 反馈时不 compact。
    // resume 加载的是已精简的活跃链，第一轮直接原样发送，等拿到真实 token 数后再决定。
    if turn_count == 1 && last_api_input_tokens == 0 {
        return None;
    }

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
        log::warn!(target: LOG_TARGET, "PreCompact hook blocked compaction");
        return None;
    }

    let should_compact = if last_api_input_tokens > 0 {
        compact::needs_compaction_actual(
            last_api_input_tokens,
            last_api_output_tokens,
            cached_tokens,
            reasoning_tokens,
            context_size,
        )
    } else {
        compact::needs_compaction_full(
            messages,
            system_prompt_text,
            context_size,
            tool_schema_tokens,
        )
    };

    if !should_compact || messages.len() <= 4 {
        return None;
    }

    let old_len = messages.len();

    // precompact reflection（记忆系统在 compact 前抢救信息）
    if let Some(text) = crate::business::chat::looping::reflection::run_precompact_reflection(
        memory_config,
        messages,
        cwd,
        llm_client.as_ref(),
        system_prompt_text,
    )
    .await
    {
        let _ = sink
            .send_event(RuntimeStreamEvent::SystemMessage(text))
            .await;
    }

    // full compact：summary + recent tail
    let result = compact::compact_messages_with_llm(
        messages,
        system_prompt_text,
        context_size,
        Some(llm_client.as_ref()),
    )
    .await?;

    let new_len = result.recent_messages.len();
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

    Some(CompactOutcome {
        summary: result.summary,
        messages: result.recent_messages,
    })
}
