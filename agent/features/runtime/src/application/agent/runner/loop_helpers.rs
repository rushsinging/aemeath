use share::string_idx::slice_head;

use share::message::Message;

use super::logging::build_json_logger_tool_result_data;
use super::loop_run::SubAgentRun;
use provider::SystemBlock;

impl<'a> SubAgentRun<'a> {
    pub(super) fn progress_tools_done(&self, turn_number: usize, result_count: usize) {
        (self.progress)(
            Some(turn_number),
            &format!(
                "Tools done ({}s elapsed), {} results",
                self.start_time.elapsed().as_secs(),
                result_count
            ),
        );
    }

    pub(super) fn log_result_summaries(
        &self,
        turn_number: usize,
        results: &[crate::application::agent::ToolExecution],
        call_info: &std::collections::HashMap<sdk::ids::ToolCallId, (String, String)>,
    ) {
        for ex in results.iter() {
            let id = &ex.call_id;
            let output = &ex.outcome.text;
            let label = if ex.outcome.is_error { "ERR" } else { "OK" };
            if let Some((name, input_short)) = call_info.get(id) {
                (self.progress)(Some(turn_number), &format!("  → {}({})", name, input_short));
            }
            let out_short = if output.len() > 300 {
                format!("{}...[{} chars]", slice_head(output, 300), output.len())
            } else {
                output.clone()
            };
            let tool_name = call_info
                .get(id)
                .map(|(name, _)| name.as_str())
                .unwrap_or("?");
            (self.progress)(
                Some(turn_number),
                &format!("  ← {}[{}]: {}", tool_name, label, out_short),
            );
        }
    }

    pub(super) fn log_tool_results(
        &self,
        _turn_number: usize,
        results: &[crate::application::agent::ToolExecution],
        call_info: &std::collections::HashMap<sdk::ids::ToolCallId, (String, String)>,
    ) {
        for ex in results.iter() {
            let data = build_json_logger_tool_result_data(
                &ex.call_id,
                &ex.outcome.text,
                ex.outcome.is_error,
                call_info,
            );
            log::debug!(
                target: crate::LOG_TARGET,
                "tool_result: {}",
                serde_json::to_string(&data).unwrap_or_default()
            );
        }
    }

    pub(super) async fn compact_now(&mut self, turn_number: usize) {
        // microcompact：规则驱动清理陈旧探索类 tool result（零 LLM 成本）。
        let mc_cleared = context::compact::microcompact_messages(&mut self.messages);
        if mc_cleared > 0 {
            log::info!(target: crate::LOG_TARGET,
                "[microcompact] sub-agent cleared {} stale tool results", mc_cleared);
        }

        let old_len = self.messages.len();
        let previous_summary =
            compact_summary_from_system_blocks(&self.system_blocks).map(str::to_owned);
        let cancellation = self.agent.ctx.cancellation();
        let cancel_token = self.runtime_cancellation.clone();
        let result = tokio::select! {
            _ = cancellation.cancelled() => None,
            result = context::compact::compact_messages_with_llm(
                &self.messages,
                previous_summary.as_deref(),
                self.ctx_context_size,
                Some(&self.client),
                None,
                &cancel_token,
            ) => result,
        };

        if let Some(result) = result {
            self.messages = result.recent_messages;
            inject_summary_into_system_blocks(&mut self.system_blocks, result.summary);
            (self.progress)(
                Some(turn_number),
                &format!(
                    "Agent compacted: {} → {} messages",
                    old_len,
                    self.messages.len()
                ),
            );
        }
    }
}

/// Sub-agent compact summary 在 system_blocks 中的标识，用于查找和替换。
const COMPACT_SUMMARY_TAG: &str = "<compact-summary>";
const COMPACT_SUMMARY_END_TAG: &str = "</compact-summary>";

pub(super) fn compact_summary_from_system_blocks(system_blocks: &[SystemBlock]) -> Option<&str> {
    system_blocks
        .iter()
        .find_map(|block| {
            block
                .text
                .strip_prefix(COMPACT_SUMMARY_TAG)
                .and_then(|text| text.strip_suffix(COMPACT_SUMMARY_END_TAG))
        })
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
}

/// 将 compact summary 注入 system_blocks（与主循环行为一致）。
///
/// - 若已有 compact summary block（含 `COMPACT_SUMMARY_TAG` 标记），**替换**之（不累积）。
/// - 否则追加一个新 block（`SystemBlock::dynamic`，不缓存）。
pub(super) fn inject_summary_into_system_blocks(
    system_blocks: &mut Vec<SystemBlock>,
    summary: String,
) {
    let block_text = format!("{COMPACT_SUMMARY_TAG}\n{summary}\n</compact-summary>");

    // 查找已有的 compact summary block 并替换
    if let Some(existing) = system_blocks
        .iter_mut()
        .find(|b| b.text.starts_with(COMPACT_SUMMARY_TAG))
    {
        existing.text = block_text;
    } else {
        system_blocks.push(SystemBlock::dynamic(block_text));
    }
}

pub(super) async fn append_tool_results(
    materializer: &crate::application::tool_result_materialization::ToolResultMaterializer,
    messages: &mut Vec<Message>,
    results: Vec<crate::application::agent::ToolExecution>,
    session_id: &str,
) {
    let provider_results: Vec<_> = results
        .into_iter()
        .map(|ex| {
            (
                ex.provider_id,
                ex.outcome.text,
                ex.outcome.data,
                ex.outcome.is_error,
                ex.outcome.images,
            )
        })
        .collect();
    messages.push(
        materializer
            .materialize_provider_results(session_id, provider_results)
            .await,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::message::ContentBlock;

    #[tokio::test]
    async fn test_append_tool_results_uses_provider_id_not_runtime_id() {
        let mut messages = Vec::new();
        let results = vec![crate::application::agent::ToolExecution::from_parts(
            sdk::ids::ToolCallId::from_legacy_or_new("runtime-id"),
            "provider-id".to_string(),
            "Bash".to_string(),
            tools::ToolOutcome::new("ok", serde_json::json!({ "text": "ok" }), Vec::new()),
        )];

        let materializer = crate::application::testing::test_tool_result_materializer();
        append_tool_results(
            materializer.as_ref(),
            &mut messages,
            results,
            "test-sub-agent-provider-id",
        )
        .await;

        let [ContentBlock::ToolResult { tool_use_id, .. }] = messages[0].content.as_slice() else {
            panic!("expected one tool result");
        };
        assert_eq!(tool_use_id, "provider-id");
    }

    #[tokio::test]
    async fn test_append_tool_results_persists_oversized_sub_agent_result() {
        const THRESHOLD: usize = 50_000;
        let session_id = format!("test-sub-agent-{}", std::process::id());
        let oversized = "x".repeat(THRESHOLD + 1);
        let mut messages = Vec::new();
        let results = vec![crate::application::agent::ToolExecution::from_parts(
            sdk::ids::ToolCallId::from_legacy_or_new("tool-oversized"),
            "provider-oversized".to_string(),
            "Bash".to_string(),
            tools::ToolOutcome::new(
                oversized,
                serde_json::json!({ "text": "oversized" }),
                Vec::new(),
            ),
        )];

        let materializer = crate::application::testing::test_tool_result_materializer();
        append_tool_results(materializer.as_ref(), &mut messages, results, &session_id).await;

        let [ContentBlock::ToolResult { content, .. }] = messages[0].content.as_slice() else {
            panic!("expected one tool result");
        };
        let content = match content {
            serde_json::Value::Object(map) => map,
            other => panic!("tool result should be json object, got {other:?}"),
        };
        let text = content
            .get("text")
            .and_then(|value| value.as_str())
            .expect("persisted reference should be in text field");
        assert!(text.contains("<persisted-output>"));
        assert!(text.len() < THRESHOLD);
        assert!(text.contains(&session_id));
    }

    // ── inject_summary_into_system_blocks ───────────────────────

    #[test]
    fn test_inject_summary_appends_new_block_when_absent() {
        let mut blocks = vec![SystemBlock::cached("system prompt".to_string())];

        inject_summary_into_system_blocks(&mut blocks, "first summary".to_string());

        assert_eq!(blocks.len(), 2);
        assert!(blocks[1].text.contains("<compact-summary>"));
        assert!(blocks[1].text.contains("first summary"));
        assert!(
            blocks[1].cache_control.is_none(),
            "summary block should not be cached"
        );
    }

    #[test]
    fn test_inject_summary_replaces_existing_block_on_second_compact() {
        let mut blocks = vec![SystemBlock::cached("system prompt".to_string())];

        inject_summary_into_system_blocks(&mut blocks, "first summary".to_string());
        inject_summary_into_system_blocks(&mut blocks, "second summary".to_string());

        assert_eq!(
            blocks.len(),
            2,
            "should not accumulate summary blocks across compactions"
        );
        assert!(blocks[1].text.contains("second summary"));
        assert!(!blocks[1].text.contains("first summary"));
    }

    #[test]
    fn test_inject_summary_preserves_original_system_block() {
        let mut blocks = vec![
            SystemBlock::cached("original system".to_string()),
            SystemBlock::cached("guidance".to_string()),
        ];

        inject_summary_into_system_blocks(&mut blocks, "summary text".to_string());

        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].text, "original system");
        assert_eq!(blocks[1].text, "guidance");
        assert!(blocks[2].text.contains("summary text"));
    }

    #[tokio::test]
    async fn sub_second_compact_passes_existing_summary_to_context() {
        let mut blocks = vec![SystemBlock::cached("system prompt".to_string())];
        inject_summary_into_system_blocks(&mut blocks, "first sub compact summary".to_string());
        let previous_summary = compact_summary_from_system_blocks(&blocks);
        let messages = (0..10)
            .map(|index| Message::user(format!("message-{index}")))
            .collect::<Vec<_>>();
        let cancel = tokio_util::sync::CancellationToken::new();

        let result = context::compact::compact_messages_with_llm(
            &messages,
            previous_summary,
            100_000,
            None,
            None,
            &cancel,
        )
        .await
        .expect("second compact should run");

        assert!(result.summary.contains("first sub compact summary"));
    }
}
