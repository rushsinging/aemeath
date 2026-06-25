use super::finalize::{finalize_sub_agent, AgentRunOutcome, AgentRunStatus};
use super::logging::{
    build_json_logger_input_data, build_json_logger_output_data, build_json_logger_tool_call_data,
};
use super::loop_helpers::append_tool_results;
use super::progress::build_tool_calls_progress_event;
use super::SilentHandler;
use crate::business::agent::Agent;
use crate::business::compact::offload_tool_result;
use crate::LOG_TARGET;
use provider::api::LlmClient;
use provider::api::{StopReason, SystemBlock};
use share::message::Message;
use share::string_idx::slice_head;
use share::tool::{AgentProgressEvent, AgentProgressKind};
use std::sync::Arc;
use tools::api::ToolExecutionContext;

#[allow(clippy::type_complexity)]
pub(super) struct SubAgentRun<'a> {
    pub prompt: &'a str,
    pub system: String,
    pub ctx: &'a ToolExecutionContext,
    pub progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    pub client: Arc<LlmClient>,
    pub hook_runner: hook::api::HookRunner,
    pub sub_schemas: Vec<serde_json::Value>,
    pub messages: Vec<Message>,
    pub handler: SilentHandler,
    pub system_blocks: Vec<SystemBlock>,
    pub log_request_messages: Box<dyn Fn(usize, &[Message]) + Send + Sync + 'a>,
    pub agent: Agent<'a>,
    pub max_turns: usize,
    pub start_time: std::time::Instant,
    pub max_duration: std::time::Duration,
    pub session_id: String,
    pub role_name_for_log: String,
    pub model_name_for_log: String,
    pub resolved_spec: Option<String>,
    pub previous_max_tokens: u32,
    pub previous_reasoning_level: provider::contract::ReasoningLevel,
    pub restore_max_tokens: bool,
    pub progress: Box<dyn Fn(Option<usize>, &str) + Send + Sync + 'a>,
    pub ctx_context_size: usize,
}

impl<'a> SubAgentRun<'a> {
    pub async fn run_loop(mut self) -> String {
        let workspace_root = self.ctx.workspace_read().current_workspace_root();
        macro_rules! finalize_and_return {
            ($status:expr, $turns:expr, $result:expr) => {{
                let outcome = AgentRunOutcome {
                    status: $status,
                    turns: $turns,
                    duration: self.start_time.elapsed(),
                    role: Some(self.role_name_for_log.clone()),
                    model: self.model_name_for_log.clone(),
                };
                finalize_sub_agent(
                    &outcome,
                    self.client.as_ref(),
                    &self.hook_runner,
                    &self.session_id,
                    self.prompt,
                    &self.system,
                    self.resolved_spec.as_deref(),
                    &$result,
                    self.previous_max_tokens,
                    self.previous_reasoning_level,
                    self.restore_max_tokens,
                    self.progress_tx.as_ref(),
                    &workspace_root,
                )
                .await;
                return $result;
            }};
        }

        for turn in 0..self.max_turns {
            let turn_number = turn + 1;
            if self.ctx.cancel.is_cancelled() {
                (self.progress)(Some(turn_number), "Agent cancelled by user");
                let result = "Cancelled by user".to_string();
                finalize_and_return!(AgentRunStatus::Cancelled, turn, result);
            }
            if self.start_time.elapsed() > self.max_duration {
                self.progress_timeout(turn_number);
                let result = self.timeout_result();
                finalize_and_return!(AgentRunStatus::TimedOut, turn, result);
            }

            self.progress_turn_start(turn_number);
            (self.log_request_messages)(turn_number, &self.messages);
            self.log_input(turn_number);

            let response = self
                .client
                .stream_message(
                    &self.system_blocks,
                    &self.messages,
                    &self.sub_schemas,
                    &mut self.handler,
                    &self.ctx.cancel,
                )
                .await;

            match response {
                Ok(resp) => {
                    let api_input = resp.usage.input_tokens as u64;
                    self.progress_api_ok(turn_number, &resp);
                    self.messages.push(resp.assistant_message.clone());
                    self.log_output(turn_number, &resp);
                    self.send_text_progress(turn, &resp);

                    let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                    if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                        (self.progress)(Some(turn_number), "Agent completed");
                        let result = resp.assistant_message.text_content();
                        finalize_and_return!(AgentRunStatus::Completed, turn + 1, result);
                    }

                    self.log_tool_calls(turn_number, &tool_calls);
                    let call_info = self.build_call_info(&tool_calls);
                    if let Some(ref tx) = self.progress_tx {
                        let _ = tx.try_send(build_tool_calls_progress_event(turn + 1, &tool_calls));
                    }

                    let mut results = self.agent.execute_tools(&tool_calls).await;
                    self.progress_tools_done(turn_number, results.len());
                    self.log_result_summaries(turn_number, &results, &call_info);
                    self.log_tool_results(turn_number, &results, &call_info);

                    // 合并 #380：offload 大结果到磁盘；改用 ToolExecution 字段访问。
                    for ex in &mut results {
                        let offloaded = offload_tool_result(
                            &ex.outcome.text,
                            ex.call_id.as_str(),
                            &self.session_id,
                        );
                        if offloaded.len() != ex.outcome.text.len() {
                            ex.outcome.text = offloaded;
                            ex.outcome.data = serde_json::json!({ "text": &ex.outcome.text });
                        }
                    }
                    append_tool_results(&mut self.messages, results, &self.session_id);
                    self.compact_if_needed(api_input, resp.usage.output_tokens as u64, turn_number)
                        .await;
                }
                Err(e) => {
                    if e.is_cancelled() || self.ctx.cancel.is_cancelled() {
                        (self.progress)(Some(turn_number), "Agent cancelled by user");
                        let result = "Cancelled by user".to_string();
                        finalize_and_return!(AgentRunStatus::Cancelled, turn, result);
                    }
                    (self.progress)(Some(turn_number), &format!("Agent error: {e}"));
                    let error_string = e.to_string();
                    let result = format!("Sub-agent error: {error_string}");
                    finalize_and_return!(AgentRunStatus::ApiError(error_string), turn, result);
                }
            }
        }

        (self.progress)(
            Some(self.max_turns),
            &format!(
                "Agent reached max turns ({}), returning partial result",
                self.max_turns
            ),
        );
        let result = self.max_turns_result();
        finalize_and_return!(AgentRunStatus::MaxTurns, self.max_turns, result);
    }

    fn progress_timeout(&self, turn_number: usize) {
        (self.progress)(
            Some(turn_number),
            &format!(
                "Agent timed out after {}s",
                self.start_time.elapsed().as_secs()
            ),
        );
    }

    fn timeout_result(&self) -> String {
        let elapsed_secs = self.start_time.elapsed().as_secs();
        self.messages
            .iter()
            .rev()
            .map(|msg| msg.text_content())
            .find(|text| !text.is_empty())
            .map(|text| format!("{}\n\n[Sub-agent timed out after {}s]", text, elapsed_secs))
            .unwrap_or_else(|| format!("Sub-agent timed out after {}s", elapsed_secs))
    }

    fn progress_turn_start(&self, turn_number: usize) {
        let msg_tokens = crate::business::compact::estimate_messages_tokens(&self.messages);
        (self.progress)(
            Some(turn_number),
            &format!(
                "Agent turn {}/{}, messages: {}, est_tokens: {}",
                turn_number,
                self.max_turns,
                self.messages.len(),
                msg_tokens
            ),
        );
    }

    fn log_input(&self, turn_number: usize) {
        let mut data = build_json_logger_input_data(
            &self.messages,
            self.system_blocks.len(),
            &self.sub_schemas,
        );
        if let serde_json::Value::Object(ref mut map) = data {
            map.insert(
                "event_type".to_string(),
                serde_json::Value::String("llm_input".to_string()),
            );
            map.insert(
                "role".to_string(),
                serde_json::Value::String(self.role_name_for_log.clone()),
            );
        }
        log::debug!(target: LOG_TARGET, "{}", serde_json::to_string(&data).unwrap_or_default());
        logging::context::set_current_turn(turn_number);
    }

    fn progress_api_ok(&self, turn_number: usize, resp: &provider::api::StreamResponse) {
        (self.progress)(
            Some(turn_number),
            &format!(
                "API ok: in={} out={} stop={:?}",
                resp.usage.input_tokens, resp.usage.output_tokens, resp.stop_reason
            ),
        );
    }

    fn log_output(&self, turn_number: usize, resp: &provider::api::StreamResponse) {
        let mut data = build_json_logger_output_data(
            resp,
            self.start_time.elapsed().as_secs_f64(),
            self.client.provider_name(),
        );
        if let serde_json::Value::Object(ref mut map) = data {
            map.insert(
                "event_type".to_string(),
                serde_json::Value::String("llm_output".to_string()),
            );
            map.insert(
                "role".to_string(),
                serde_json::Value::String(self.role_name_for_log.clone()),
            );
        }
        log::debug!(target: LOG_TARGET, "{}", serde_json::to_string(&data).unwrap_or_default());
        logging::context::set_current_turn(turn_number);
    }

    fn send_text_progress(&self, turn: usize, resp: &provider::api::StreamResponse) {
        if let Some(ref tx) = self.progress_tx {
            let text = resp.assistant_message.text_content();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let short = if trimmed.len() > 300 {
                    format!("{}...", slice_head(trimmed, 300))
                } else {
                    trimmed.to_string()
                };
                let _ = tx.try_send(AgentProgressEvent {
                    sequence: turn + 1,
                    kind: AgentProgressKind::Message { text: short },
                });
            }
        }
    }

    fn log_tool_calls(&self, turn_number: usize, tool_calls: &[crate::business::agent::ToolCall]) {
        for tool_call in tool_calls {
            let data = build_json_logger_tool_call_data(tool_call);
            log::debug!(
                target: LOG_TARGET,
                "tool_call: {}",
                serde_json::to_string(&data).unwrap_or_default()
            );
        }
        logging::context::set_current_turn(turn_number);
    }

    fn build_call_info(
        &self,
        tool_calls: &[crate::business::agent::ToolCall],
    ) -> std::collections::HashMap<sdk::ids::ToolCallId, (String, String)> {
        tool_calls
            .iter()
            .map(|call| {
                let input_summary = call.input.to_string();
                let input_short = if input_summary.len() > 200 {
                    format!("{}...", slice_head(&input_summary, 200))
                } else {
                    input_summary
                };
                (call.id.clone(), (call.name.clone(), input_short))
            })
            .collect()
    }
}
