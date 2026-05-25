use super::compact;
use super::{LlmClient, Message, SilentCompactHandler, SystemBlock, TerminalRenderer};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Shared compaction logic used in both outer and inner loop.
pub(super) async fn compact_messages_inner(
    messages: &mut Vec<Message>,
    system_prompt_text: &str,
    context_size: usize,
    client: &LlmClient,
    hook_runner: &::runtime::api::hook::hook::HookRunner,
    turn_count: usize,
    compact_state: &mut compact::AutoCompactState,
    read_files: &Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
) {
    let old_len = messages.len();
    let (blocked, pre_results) = hook_runner.pre_compact(turn_count, old_len).await;
    for result in &pre_results {
        if let Some(error) = &result.error {
            log::warn!("PreCompact hook error: {error}");
        }
        if !result.output.trim().is_empty() {
            eprintln!("{}", result.output.trim());
        }
    }
    if blocked {
        log::warn!("PreCompact hook blocked compaction");
        return;
    }

    let keep_recent = (old_len * 40 / 100).max(4).min(old_len - 1);
    let split_point = old_len - keep_recent;
    let early_messages = &messages[..split_point];

    let compact_request = compact::build_compact_request(early_messages);
    let compact_system = vec![SystemBlock::dynamic(
        "You are a conversation summarizer. Respond only with the summary.".to_string(),
    )];
    let mut silent_handler = SilentCompactHandler;
    let compact_cancel = CancellationToken::new();
    match client
        .stream_message(
            &compact_system,
            &compact_request,
            &[],
            &mut silent_handler,
            &compact_cancel,
        )
        .await
    {
        Ok(compact_resp) => {
            let summary =
                compact::parse_compact_response(&compact_resp.assistant_message.text_content());
            let recent = messages[split_point..].to_vec();
            let files = read_files.lock().unwrap().clone();
            let (compacted, _) =
                compact::assemble_compacted_with_files(summary, &recent, split_point, Some(&files));
            *messages = compacted;
            compact_state.record_success();
            TerminalRenderer::print_compaction(old_len, messages.len());
            log_post_compact_results(
                hook_runner
                    .post_compact(turn_count, old_len, messages.len())
                    .await,
            );
        }
        Err(_) => {
            compact_state.record_failure();
            let (compacted, was_compacted) =
                compact::compact_messages(messages, system_prompt_text, context_size);
            if was_compacted {
                *messages = compacted;
                TerminalRenderer::print_compaction(old_len, messages.len());
                log_post_compact_results(
                    hook_runner
                        .post_compact(turn_count, old_len, messages.len())
                        .await,
                );
            }
        }
    }
}

fn log_post_compact_results(results: Vec<::runtime::api::hook::hook::HookResult>) {
    for result in results {
        if let Some(error) = result.error {
            log::warn!("PostCompact hook error: {error}");
        }
        if !result.output.trim().is_empty() {
            eprintln!("{}", result.output.trim());
        }
    }
}
