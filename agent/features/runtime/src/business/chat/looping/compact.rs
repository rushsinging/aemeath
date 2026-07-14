use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::{ChatEventSink, CompactStage, RuntimeStreamEvent};
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
    language: &str,
    workspace_root: &std::path::Path,
    cancel: &tokio_util::sync::CancellationToken,
) -> Option<CompactOutcome>
where
    S: ChatEventSink,
{
    use context::compact;

    // resume 保护：首 turn 无 API 反馈时不 compact。
    // resume 加载的是已精简的活跃链，第一轮直接原样发送，等拿到真实 token 数后再决定。
    if turn_count == 1 && last_api_input_tokens == 0 {
        return None;
    }

    // PreCompact hook
    let pre_compact_results = hook_ui
        .run_json_with_cancel(
            hook_runner,
            HookEvent::PreCompact,
            None,
            HookData::Compact(CompactHookData {
                turns: turn_count,
                messages_before: messages.len(),
                messages_after: None,
                was_compacted: false,
            }),
            workspace_root,
            cancel,
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
        language,
    )
    .await
    {
        let _ = sink
            .send_event(RuntimeStreamEvent::SystemMessage(text))
            .await;
    }

    // full compact：summary + recent tail
    let progress = make_progress_sink(sink);

    let result = compact::compact_messages_with_llm(
        messages,
        system_prompt_text,
        context_size,
        Some(llm_client.as_ref()),
        Some(progress.as_ref()),
        cancel,
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
        .run_json_with_cancel(
            hook_runner,
            HookEvent::PostCompact,
            None,
            HookData::Compact(CompactHookData {
                turns: turn_count,
                messages_before: old_len,
                messages_after: Some(new_len),
                was_compacted: true,
            }),
            workspace_root,
            cancel,
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

/// 构造一个通过 `ChatEventSink::try_send_event` 发送 `CompactProgress` 事件的进度回调。
fn make_progress_sink<S: ChatEventSink>(sink: &S) -> Box<dyn context::compact::CompactProgressFn> {
    struct SinkProgress<S: ChatEventSink> {
        sink: S,
    }
    impl<S: ChatEventSink> context::compact::CompactProgressFn for SinkProgress<S> {
        fn emit(&self, stage: CompactStage, current: Option<usize>, total: Option<usize>) {
            self.sink
                .try_send_event(RuntimeStreamEvent::CompactProgress {
                    stage,
                    current,
                    total,
                });
        }
    }
    Box::new(SinkProgress { sink: sink.clone() })
}

/// 手动 compact（`/compact` 触发）：无条件执行压缩（绕过 token 阈值检查）。
///
/// 与 `auto_compact` 共享 PreCompact/PostCompact hook + precompact reflection + 进度事件。
/// 返回 `Some(CompactOutcome)` 表示发生了压缩；`None` 表示无需压缩（消息太少）。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn manual_compact<S>(
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &HookRunner,
    turn_count: usize,
    messages: &[Message],
    system_prompt_text: &str,
    context_size: usize,
    memory_config: &share::config::MemoryConfig,
    cwd: &std::path::Path,
    llm_client: &Arc<provider::api::LlmClient>,
    language: &str,
    workspace_root: &std::path::Path,
) -> Option<CompactOutcome>
where
    S: ChatEventSink,
{
    use context::compact;

    if messages.len() <= 4 {
        let _ = sink
            .send_event(RuntimeStreamEvent::SystemMessage(
                "Not enough messages to compact.".to_string(),
            ))
            .await;
        return None;
    }

    // Manual compact is an idle command outside an active Run, so it owns its command scope.
    let manual_cancel = tokio_util::sync::CancellationToken::new();
    // PreCompact hook
    let pre_compact_results = hook_ui
        .run_json_with_cancel(
            hook_runner,
            HookEvent::PreCompact,
            None,
            HookData::Compact(CompactHookData {
                turns: turn_count,
                messages_before: messages.len(),
                messages_after: None,
                was_compacted: false,
            }),
            workspace_root,
            &manual_cancel,
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
        log::warn!(target: LOG_TARGET, "PreCompact hook blocked manual compaction");
        return None;
    }

    let old_len = messages.len();

    // precompact reflection
    if let Some(text) = crate::business::chat::looping::reflection::run_precompact_reflection(
        memory_config,
        messages,
        cwd,
        llm_client.as_ref(),
        system_prompt_text,
        language,
    )
    .await
    {
        let _ = sink
            .send_event(RuntimeStreamEvent::SystemMessage(text))
            .await;
    }

    // full compact：summary + recent tail（手动场景绕过 token 阈值不太合适，
    // 但 compact_messages_with_llm 内部的 needs_compaction 会基于 system_prompt + context_size
    // 判断；手动 compact 时用户明确要求，若消息太少会返回 None）。
    let progress = make_progress_sink(sink);
    let manual_cancel = tokio_util::sync::CancellationToken::new();

    let result = compact::compact_messages_with_llm(
        messages,
        system_prompt_text,
        context_size,
        Some(llm_client.as_ref()),
        Some(progress.as_ref()),
        &manual_cancel,
    )
    .await?;

    let new_len = result.recent_messages.len();
    let _ = sink
        .send_event(RuntimeStreamEvent::SystemMessage(format!(
            "[compacted: {} → {} messages]",
            old_len, new_len
        )))
        .await;

    // PostCompact hook
    let post_compact_results = hook_ui
        .run_json_with_cancel(
            hook_runner,
            HookEvent::PostCompact,
            None,
            HookData::Compact(CompactHookData {
                turns: turn_count,
                messages_before: old_len,
                messages_after: Some(new_len),
                was_compacted: true,
            }),
            workspace_root,
            &manual_cancel,
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
