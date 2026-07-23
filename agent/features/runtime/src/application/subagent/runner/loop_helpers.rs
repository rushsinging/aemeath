use share::string_idx::slice_head;

use share::message::Message;

use super::logging::build_json_logger_tool_result_data;
use super::loop_run::SubAgentRun;

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
        results: &[crate::application::subagent::ToolExecution],
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
        results: &[crate::application::subagent::ToolExecution],
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
}

pub(super) async fn append_tool_results(
    materializer: &crate::application::tool_result_materialization::ToolResultMaterializer,
    messages: &mut Vec<Message>,
    results: Vec<crate::application::subagent::ToolExecution>,
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
        let results = vec![crate::application::subagent::ToolExecution::from_parts(
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
        let results = vec![crate::application::subagent::ToolExecution::from_parts(
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
}
