use super::finalize::{finalize_sub_agent, AgentRunOutcome, AgentRunStatus};
use super::logging::{
    build_json_logger_input_data, build_json_logger_output_data, build_json_logger_tool_call_data,
};
use super::loop_helpers::append_tool_results;
use super::progress::build_tool_calls_progress_event;
use super::SilentHandler;
use crate::business::agent::Agent;
use crate::business::agent_run::{Run, RunDomainEvent, RunSpec};
use crate::business::loop_engine::{
    run_loop as shared_run_loop, LoopEngineError, ModelStep, RunLoopPort, ToolGuardDecision,
    ToolStep,
};
use crate::LOG_TARGET;
use async_trait::async_trait;
use provider::api::LlmClient;
use provider::api::{StopReason, SystemBlock};
use share::message::Message;
use share::string_idx::slice_head;
use share::tool::{AgentProgressEvent, AgentProgressKind};
use std::sync::Arc;
use tools::api::AgentRunTerminal;

struct ActiveRunRegistration {
    active_run: Arc<dyn crate::business::agent_run::ActiveRunPort>,
    run_id: sdk::RunId,
}

impl ActiveRunRegistration {
    fn new(
        active_run: Arc<dyn crate::business::agent_run::ActiveRunPort>,
        run_id: sdk::RunId,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Self {
        active_run.activate(run_id.clone(), cancel);
        Self { active_run, run_id }
    }
}

impl Drop for ActiveRunRegistration {
    fn drop(&mut self) {
        self.active_run.clear(&self.run_id);
    }
}

pub(super) fn messages_for_llm(messages: &[Message]) -> Vec<Message> {
    messages.iter().map(Message::to_llm_view).collect()
}

#[allow(clippy::type_complexity)]
pub(super) struct SubAgentRun<'a> {
    pub prompt: &'a str,
    pub system: String,
    pub progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    pub client: Arc<LlmClient>,
    pub _shared_client_guard: Option<tokio::sync::OwnedMutexGuard<()>>,
    pub hook_runner: hook::api::HookRunner,
    pub sub_schemas: Vec<serde_json::Value>,
    pub messages: Vec<Message>,
    pub handler: SilentHandler,
    pub system_blocks: Vec<SystemBlock>,
    pub log_request_messages: Box<dyn Fn(usize, &[Message]) + Send + Sync + 'a>,
    pub agent: Agent<'a>,
    pub timeout: std::time::Duration,
    pub turn_count: usize,
    pub last_api_input_tokens: u64,
    pub last_api_output_tokens: u64,
    pub active_run: Arc<dyn crate::business::agent_run::ActiveRunPort>,
    pub terminal: Option<AgentRunTerminal>,
    pub start_time: std::time::Instant,
    pub session_id: String,
    pub run_id: sdk::RunId,
    pub parent_run_id: Option<sdk::RunId>,
    pub role_name_for_log: String,
    pub model_name_for_log: String,
    pub resolved_spec: Option<String>,
    pub previous_max_tokens: u32,
    pub previous_reasoning_level: provider::contract::ReasoningLevel,
    pub restore_max_tokens: bool,
    pub progress: Box<dyn Fn(Option<usize>, &str) + Send + Sync + 'a>,
    pub token_budget: context::api::compact::TokenBudgetConfig,
}

impl<'a> SubAgentRun<'a> {
    /// Runs a sub-agent through the same loop engine used by every agent run.
    pub async fn run_loop(mut self) -> AgentRunTerminal {
        let mut run = Run::with_id(
            self.run_id.clone(),
            RunSpec::sub(self.role_name_for_log.clone(), self.timeout),
            self.parent_run_id.clone(),
        );
        let cancel = self.agent.ctx.cancel.clone();
        let _registration = ActiveRunRegistration::new(
            self.active_run.clone(),
            self.run_id.clone(),
            cancel.clone(),
        );
        let loop_result = shared_run_loop(&mut run, &cancel, &mut self).await;

        // A normal terminal path is recorded by `emit` from the authoritative
        // RunDomainEvent. Keep an infrastructure fallback so finalization still
        // runs if the engine itself cannot finish a transition.
        let terminal = self
            .terminal
            .take()
            .unwrap_or_else(|| AgentRunTerminal::Failed {
                error: loop_result
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| {
                        "shared run loop ended without a terminal event".to_string()
                    }),
            });

        let turns = run
            .steps()
            .iter()
            .filter(|step| step.status() == crate::business::agent_run::RunStepStatus::Done)
            .count();
        let status = match &terminal {
            AgentRunTerminal::Completed { .. } => AgentRunStatus::Completed,
            AgentRunTerminal::Failed { error } => {
                if error.starts_with("run timed out after ") {
                    AgentRunStatus::TimedOut
                } else {
                    AgentRunStatus::Failed(error.clone())
                }
            }
            AgentRunTerminal::Cancelled => AgentRunStatus::Cancelled,
        };
        let outcome = AgentRunOutcome {
            status,
            turns,
            duration: self.start_time.elapsed(),
            role: Some(self.role_name_for_log.clone()),
            model: self.model_name_for_log.clone(),
        };
        let workspace_root = self.agent.ctx.workspace_read().current_workspace_root();
        let output = terminal.output();
        finalize_sub_agent(
            &outcome,
            self.client.as_ref(),
            &self.hook_runner,
            &self.session_id,
            self.prompt,
            &self.system,
            self.resolved_spec.as_deref(),
            &output,
            self.previous_max_tokens,
            self.previous_reasoning_level,
            self.restore_max_tokens,
            self.progress_tx.as_ref(),
            &workspace_root,
        )
        .await;

        terminal
    }

    fn progress_turn_start(&self, turn_number: usize) {
        let msg_tokens = context::api::compact::estimate_messages_tokens(&self.messages);
        (self.progress)(
            Some(turn_number),
            &format!(
                "Agent turn {}, messages: {}, est_tokens: {}",
                turn_number,
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
                    sequence: turn,
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

fn terminal_from_domain_event(event: &RunDomainEvent) -> Option<AgentRunTerminal> {
    match event {
        RunDomainEvent::Completed { result, .. } => Some(AgentRunTerminal::Completed {
            result: result.clone(),
        }),
        RunDomainEvent::Failed { error, .. } => Some(AgentRunTerminal::Failed {
            error: error.clone(),
        }),
        RunDomainEvent::Cancelled { .. } => Some(AgentRunTerminal::Cancelled),
        RunDomainEvent::Started { .. }
        | RunDomainEvent::StepStarted { .. }
        | RunDomainEvent::StepCompleted { .. }
        | RunDomainEvent::CancellationRequested { .. }
        | RunDomainEvent::AwaitingUser { .. }
        | RunDomainEvent::Resumed { .. }
        | RunDomainEvent::StuckDetected { .. } => None,
    }
}

#[async_trait]
impl RunLoopPort for SubAgentRun<'_> {
    async fn drain_input(
        &mut self,
    ) -> Result<Vec<crate::business::loop_engine::LoopInput>, LoopEngineError> {
        Ok(Vec::new())
    }

    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        if self.last_api_input_tokens == 0 && self.last_api_output_tokens == 0 {
            return Ok(false);
        }
        Ok(context::api::compact::needs_compaction_actual(
            self.last_api_input_tokens,
            self.last_api_output_tokens,
            &self.token_budget,
        ))
    }

    async fn compact(
        &mut self,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(), LoopEngineError> {
        let input = self.last_api_input_tokens;
        let output = self.last_api_output_tokens;
        self.compact_if_needed(input, output, self.turn_count).await;
        if !self.agent.ctx.cancel.is_cancelled() {
            self.last_api_input_tokens = 0;
            self.last_api_output_tokens = 0;
        }
        // The engine owns cancellation transitions. Returning success lets its
        // post-compaction cancellation check emit the authoritative event.
        Ok(())
    }

    async fn invoke_model(
        &mut self,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(ModelStep, crate::business::loop_engine::StepTokenUsage), LoopEngineError> {
        use crate::business::loop_engine::StepTokenUsage;
        self.turn_count += 1;
        let turn_number = self.turn_count;
        self.progress_turn_start(turn_number);
        (self.log_request_messages)(turn_number, &self.messages);
        self.log_input(turn_number);

        // Memory is queried dynamically on every turn, matching the main loop.
        let mut effective_blocks = self.system_blocks.clone();
        let memory_root = self.agent.ctx.workspace_read().initial_cwd();
        let mc = &self.agent.ctx.resources.memory_config;
        if mc.enabled && mc.inject_count > 0 {
            if let Some(block) = crate::business::chat::looping::memory_inject::build_memory_block(
                &memory_root,
                mc.inject_count,
            ) {
                effective_blocks.push(block);
            }
        }

        let messages_for_api = messages_for_llm(&self.messages);
        let response = self
            .client
            .stream_message(
                &effective_blocks,
                &messages_for_api,
                &self.sub_schemas,
                &mut self.handler,
                &self.agent.ctx.cancel,
            )
            .await;

        let resp = match response {
            Ok(resp) => resp,
            Err(error) if error.is_cancelled() || self.agent.ctx.cancel.is_cancelled() => {
                self.agent.ctx.cancel.cancel();
                return Err(LoopEngineError::Cancelled);
            }
            Err(error) => return Err(LoopEngineError::Adapter(error.to_string())),
        };

        self.last_api_input_tokens = resp.usage.input_tokens as u64;
        self.last_api_output_tokens = resp.usage.output_tokens as u64;
        self.progress_api_ok(turn_number, &resp);

        let usage = StepTokenUsage {
            input_tokens: resp.usage.input_tokens as u64,
            output_tokens: resp.usage.output_tokens as u64,
            cached_tokens: resp.usage.cached_tokens.map(u64::from).unwrap_or(0),
            cache_creation_tokens: resp.usage.cache_creation_tokens.map(u64::from).unwrap_or(0),
            reasoning_tokens: resp.usage.reasoning_tokens.map(u64::from).unwrap_or(0),
            total_tokens: resp.usage.total_tokens.map(u64::from).unwrap_or(0),
            context_window: self.ctx_context_size as u64,
            est_system_tokens: effective_blocks
                .iter()
                .map(|b| context::api::compact::estimate_tokens(&b.text))
                .sum(),
            est_tool_tokens: context::api::compact::estimate_tool_schemas_tokens(&self.sub_schemas),
            est_message_tokens: context::api::compact::estimate_messages_tokens(&messages_for_api),
        };

        self.messages.push(resp.assistant_message.clone());
        self.log_output(turn_number, &resp);
        self.send_text_progress(turn_number, &resp);

        let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
        if resp.stop_reason == StopReason::MaxTokens {
            log::warn!(
                target: crate::LOG_TARGET,
                "turn {}: 模型响应触发 max_tokens 限制，注入分块提示",
                turn_number,
            );
            self.messages.push(Message::user(
                "[系统提示] 你的上一次响应触达了 max_tokens 限制，输出被截断。\
                 请基于已有内容继续，或用更紧凑的方式重新组织响应：\
                 大文件改用 Edit 分块写入（每次 < 12k 字符），\
                 长命令用 Bash heredoc 分段执行。\
                 不要重复已输出的内容，直接从截断点继续。"
                    .to_string(),
            ));
            if tool_calls.is_empty() {
                // Preserve the old retry path: a text-only truncation did not
                // trigger compaction before asking the model to continue.
                self.last_api_input_tokens = 0;
                self.last_api_output_tokens = 0;
                // An empty tool phase advances the shared state machine while
                // retaining the old behavior of retrying a truncated response.
                return Ok((
                    ModelStep::Tools {
                        text: resp.assistant_message.text_content(),
                        calls: Vec::new(),
                    },
                    usage,
                ));
            }
        }

        if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
            return Ok((
                ModelStep::Complete {
                    text: resp.assistant_message.text_content(),
                },
                usage,
            ));
        }

        Ok((
            ModelStep::Tools {
                text: resp.assistant_message.text_content(),
                calls: tool_calls,
            },
            usage,
        ))
    }

    async fn execute_tools(
        &mut self,
        calls: &[(crate::business::agent::ToolCall, ToolGuardDecision)],
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        if calls.is_empty() {
            return Ok(ToolStep::Continue);
        }
        let allowed: Vec<_> = calls
            .iter()
            .filter_map(|(call, decision)| {
                matches!(decision, ToolGuardDecision::Allow).then_some(call.clone())
            })
            .collect();
        let mut results: Vec<_> = calls
            .iter()
            .filter_map(|(call, decision)| match decision {
                ToolGuardDecision::SoftBlock { reason } => Some(
                    crate::business::chat::looping::tool_fuse::blocked_tool_execution(call, reason),
                ),
                ToolGuardDecision::Allow => None,
            })
            .collect();

        let turn_number = self.turn_count;
        let all_calls: Vec<_> = calls.iter().map(|(call, _)| call.clone()).collect();
        self.log_tool_calls(turn_number, &all_calls);
        let call_info = self.build_call_info(&all_calls);
        if let Some(ref tx) = self.progress_tx {
            let _ = tx.try_send(build_tool_calls_progress_event(turn_number, &allowed));
        }

        let mut executed = tokio::select! {
            _ = self.agent.ctx.cancel.cancelled() => {
                return Err(LoopEngineError::Cancelled);
            }
            executed = self.agent.execute_tools(&allowed) => executed,
        };
        results.append(&mut executed);
        let mut by_id: std::collections::HashMap<_, _> = results
            .into_iter()
            .map(|result| (result.call_id.clone(), result))
            .collect();
        let results: Vec<_> = calls
            .iter()
            .filter_map(|(call, _)| by_id.remove(&call.id))
            .collect();
        self.progress_tools_done(turn_number, results.len());
        self.log_result_summaries(turn_number, &results, &call_info);
        self.log_tool_results(turn_number, &results, &call_info);
        append_tool_results(&mut self.messages, results, &self.session_id);
        Ok(ToolStep::Continue)
    }

    async fn on_stuck(
        &mut self,
        decision: &crate::business::loop_engine::StuckDecision,
    ) -> Result<(), LoopEngineError> {
        (self.progress)(Some(self.turn_count), &format!("StuckGuard: {decision:?}"));
        Ok(())
    }

    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_terminal(run_id)
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_cancellation(run_id)
    }

    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        for event in events {
            if let Some(terminal) = terminal_from_domain_event(&event) {
                match &terminal {
                    AgentRunTerminal::Completed { .. } => {
                        (self.progress)(Some(self.turn_count), "Agent completed");
                    }
                    AgentRunTerminal::Failed { error } => {
                        (self.progress)(Some(self.turn_count), &format!("Agent error: {error}"));
                    }
                    AgentRunTerminal::Cancelled => {
                        (self.progress)(Some(self.turn_count), "Agent cancelled by user");
                    }
                }
                self.terminal = Some(terminal);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{messages_for_llm, terminal_from_domain_event, ActiveRunRegistration};
    use crate::business::agent_run::{ActiveRunPort, RunDomainEvent, RunId};
    use share::message::{ContentBlock, Message, Role};

    #[derive(Default)]
    struct RecordingActiveRunPort {
        active: std::sync::Mutex<std::collections::HashSet<RunId>>,
    }

    impl ActiveRunPort for RecordingActiveRunPort {
        fn activate(&self, run_id: RunId, _cancel: tokio_util::sync::CancellationToken) {
            self.active.lock().unwrap().insert(run_id);
        }

        fn claim_terminal(&self, _run_id: &RunId) -> bool {
            true
        }

        fn claim_cancellation(&self, _run_id: &RunId) -> bool {
            true
        }

        fn clear(&self, run_id: &RunId) {
            self.active.lock().unwrap().remove(run_id);
        }
    }

    #[test]
    fn active_run_registration_clears_registry_when_dropped() {
        let registry = std::sync::Arc::new(RecordingActiveRunPort::default());
        let run_id = RunId::new_v7();
        {
            let _registration = ActiveRunRegistration::new(
                registry.clone(),
                run_id.clone(),
                tokio_util::sync::CancellationToken::new(),
            );
            assert!(registry.active.lock().unwrap().contains(&run_id));
        }
        assert!(registry.active.lock().unwrap().is_empty());
    }

    #[test]
    fn terminal_domain_events_project_to_all_agent_terminal_variants() {
        let run_id = RunId::new_v7();
        let parent_run_id = Some(RunId::new_v7());
        let cases = [
            (
                RunDomainEvent::Completed {
                    run_id: run_id.clone(),
                    parent_run_id: parent_run_id.clone(),
                    result: "done".to_string(),
                },
                Some(tools::api::AgentRunTerminal::Completed {
                    result: "done".to_string(),
                }),
            ),
            (
                RunDomainEvent::Failed {
                    run_id: run_id.clone(),
                    parent_run_id: parent_run_id.clone(),
                    error: "boom".to_string(),
                },
                Some(tools::api::AgentRunTerminal::Failed {
                    error: "boom".to_string(),
                }),
            ),
            (
                RunDomainEvent::Cancelled {
                    run_id,
                    parent_run_id,
                },
                Some(tools::api::AgentRunTerminal::Cancelled),
            ),
        ];

        for (event, expected) in cases {
            assert_eq!(terminal_from_domain_event(&event), expected);
        }
    }

    #[test]
    fn nonterminal_domain_event_does_not_create_agent_terminal() {
        let event = RunDomainEvent::Started {
            run_id: RunId::new_v7(),
            parent_run_id: Some(RunId::new_v7()),
        };

        assert_eq!(terminal_from_domain_event(&event), None);
    }

    #[test]
    fn messages_for_llm_converts_structured_tool_result_to_text() {
        let messages = vec![Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tool_1".to_string(),
                content: serde_json::json!({"stdout": "structured output"}),
                is_error: false,
                text: Some("plain output".to_string()),
            }],
            metadata: None,
        }];

        let api_messages = messages_for_llm(&messages);

        let ContentBlock::ToolResult { content, text, .. } = &api_messages[0].content[0] else {
            panic!("expected tool result");
        };
        assert_eq!(content, "plain output");
        assert!(text.is_none());
        let ContentBlock::ToolResult {
            content: original_content,
            text: original_text,
            ..
        } = &messages[0].content[0]
        else {
            panic!("expected original tool result");
        };
        assert_eq!(
            original_content,
            &serde_json::json!({"stdout": "structured output"})
        );
        assert_eq!(original_text.as_deref(), Some("plain output"));
    }
}
