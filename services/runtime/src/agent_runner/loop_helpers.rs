use crate::api::compact::safe_slice;
use crate::api::core::message::Message;

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
        results: &[crate::api::agent::ToolResultTuple],
        call_info: &std::collections::HashMap<String, (String, String)>,
    ) {
        for (id, output, is_error, _) in results.iter() {
            let label = if *is_error { "ERR" } else { "OK" };
            if let Some((name, input_short)) = call_info.get(id.as_str()) {
                (self.progress)(Some(turn_number), &format!("  → {}({})", name, input_short));
            }
            let out_short = if output.len() > 300 {
                format!("{}...[{} chars]", safe_slice(output, 300), output.len())
            } else {
                output.clone()
            };
            let tool_name = call_info
                .get(id.as_str())
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
        turn_number: usize,
        results: &[crate::api::agent::ToolResultTuple],
        call_info: &std::collections::HashMap<String, (String, String)>,
    ) {
        if let Some(ref jl) = self.runner.json_logger {
            for (id, output, is_error, _) in results.iter() {
                let data = build_json_logger_tool_result_data(id, output, *is_error, call_info);
                let _ = jl.lock().unwrap().log_tool_result(
                    turn_number,
                    &self.role_name_for_log,
                    &self.model_name_for_log,
                    data,
                );
            }
        }
    }

    pub(super) fn compact_if_needed(&mut self, api_input: u64, turn_number: usize) {
        let ctx_pct = api_input * 100 / self.ctx_context_size as u64;
        let urgency = if ctx_pct >= 50 {
            2
        } else if ctx_pct >= 35 {
            1
        } else {
            0
        };

        if urgency >= 2 {
            let old_len = self.messages.len();
            let (compacted, was_compacted) = crate::api::compact::compact_messages(
                &self.messages,
                &self.system,
                self.ctx_context_size,
            );
            if was_compacted {
                self.messages = compacted;
                (self.progress)(
                    Some(turn_number),
                    &format!(
                        "Agent compacted: {} → {} messages",
                        old_len,
                        self.messages.len()
                    ),
                );
            }
        } else if urgency >= 1 {
            crate::api::compact::microcompact(&mut self.messages, 4);
            (self.progress)(Some(turn_number), "Agent microcompacted");
        }
    }

    pub(super) fn max_turns_result(&self) -> String {
        self.messages
            .iter()
            .rev()
            .map(|msg| msg.text_content())
            .find(|text| !text.is_empty())
            .map(|text| {
                format!(
                    "{}\n\n[Sub-agent reached max turns ({})]",
                    text, self.max_turns
                )
            })
            .unwrap_or_else(|| format!("Sub-agent reached max turns ({})", self.max_turns))
    }
}

pub(super) fn append_tool_results(
    messages: &mut Vec<Message>,
    mut results: Vec<crate::api::agent::ToolResultTuple>,
    session_id: &str,
) {
    crate::api::storage::tool_result_storage::persist_oversized_results(session_id, &mut results);
    let has_images = results.iter().any(|(_, _, _, imgs)| !imgs.is_empty());
    if has_images {
        messages.push(Message::tool_results_rich(results));
    } else {
        let simple = results
            .into_iter()
            .map(|(id, output, is_error, _)| (id, output, is_error))
            .collect();
        messages.push(Message::tool_results(simple));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::compact::MAX_TOOL_RESULT_CHARS;
    use crate::api::core::message::ContentBlock;

    #[test]
    fn test_append_tool_results_persists_oversized_sub_agent_result() {
        let session_id = format!("test-sub-agent-{}", std::process::id());
        let oversized = "x".repeat(MAX_TOOL_RESULT_CHARS + 1);
        let mut messages = Vec::new();
        let results = vec![("tool-oversized".to_string(), oversized, false, Vec::new())];

        append_tool_results(&mut messages, results, &session_id);

        let [ContentBlock::ToolResult { content, .. }] = messages[0].content.as_slice() else {
            panic!("expected one tool result");
        };
        let text = content.as_str().expect("tool result should be string");
        assert!(text.contains("<persisted-output>"));
        assert!(text.len() < MAX_TOOL_RESULT_CHARS);
        assert!(text.contains(&session_id));
    }
}
