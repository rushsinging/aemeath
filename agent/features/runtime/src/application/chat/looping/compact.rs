use crate::application::chat::looping::hook_ui::HookUi;
use crate::application::chat::looping::{ChatEventSink, CompactStage, RuntimeStreamEvent};
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
use context::compact;

async fn run_full_compact(
    messages: &[Message],
    previous_summary: Option<&str>,
    context_size: usize,
    client: Option<&provider::LlmClient>,
    progress: Option<&dyn compact::CompactProgressFn>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Option<compact::CompactResult> {
    compact::compact_messages_with_llm(
        messages,
        previous_summary,
        context_size,
        client,
        progress,
        cancel,
    )
    .await
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
    previous_summary: Option<&str>,
    system_prompt_text: &str,
    context_size: usize,
    memory_config: &share::config::MemoryConfig,
    memory: &Arc<dyn memory::api::MemoryPort>,
    reflection_history: &Arc<dyn memory::api::ReflectionHistoryStore>,
    reflection_tasks: &crate::application::reflection::ReflectionTaskAdapter,
    llm_client: &Arc<provider::LlmClient>,
    language: &str,
    workspace_root: &std::path::Path,
) -> Option<CompactOutcome>
where
    S: ChatEventSink,
{
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
        log::warn!(target: crate::LOG_TARGET, "PreCompact hook blocked manual compaction");
        return None;
    }

    let old_len = messages.len();
    // Freeze the would-be-discarded messages now; submit only after a successful compact.
    let discarded_snapshot = context::compact::messages_selected_for_precompact_memory(messages);

    // full compact：summary + recent tail。手动场景由用户明确触发，绕过 token 阈值；
    // 消息太少时 compact_messages_with_llm 返回 None。
    let progress = make_progress_sink(sink);
    let manual_cancel = tokio_util::sync::CancellationToken::new();

    let result = run_full_compact(
        messages,
        previous_summary,
        context_size,
        Some(llm_client.as_ref()),
        Some(progress.as_ref()),
        &manual_cancel,
    )
    .await?;

    crate::application::chat::looping::reflection::submit_precompact_reflection_snapshot(
        reflection_tasks,
        memory_config,
        discarded_snapshot,
        llm_client,
        system_prompt_text,
        language,
        memory,
        reflection_history,
    );

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

#[cfg(test)]
mod tests {
    use super::run_full_compact;
    use share::message::Message;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn main_second_compact_passes_previous_summary_to_context() {
        let messages = (0..10)
            .map(|index| Message::user(format!("message-{index}")))
            .collect::<Vec<_>>();
        let cancel = CancellationToken::new();

        let result = run_full_compact(
            &messages,
            Some("first main compact summary"),
            100_000,
            None,
            None,
            &cancel,
        )
        .await
        .expect("second compact should run");

        assert!(result.summary.contains("first main compact summary"));
    }
}
