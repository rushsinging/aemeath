use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, CompactionDecision, ContextAppend, ContextAppendError, ContextPortError,
    ContextRequest, ContextWindow, InvocationReminder, ManualCompactRequest, SessionId,
    SystemBlock, TaskProgressStatus, ToolReceiptMutation, ToolReceiptMutationError,
    ToolReceiptMutationReceipt,
};
use crate::ports::{ContextMemorySource, ContextPort, ContextPromptSource, SessionRepository};

pub struct ContextApplicationService {
    session: Arc<dyn SessionRepository>,
    prompt: Arc<dyn ContextPromptSource>,
    memory: Arc<dyn ContextMemorySource>,
}

impl ContextApplicationService {
    pub fn new(
        session: Arc<dyn SessionRepository>,
        prompt: Arc<dyn ContextPromptSource>,
        memory: Arc<dyn ContextMemorySource>,
    ) -> Self {
        Self {
            session,
            prompt,
            memory,
        }
    }

    async fn build_candidate(
        &self,
        request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError> {
        #[cfg(test)]
        let build_started = std::time::Instant::now();
        #[cfg(test)]
        let snapshot_started = std::time::Instant::now();
        let snapshot = self
            .session
            .snapshot(&request.session_id)
            .await
            .map_err(ContextPortError::SessionRepository)?;
        #[cfg(test)]
        {
            let (snapshot_committed_steps, snapshot_shared_messages) =
                snapshot.messages.shared_backing_metrics();
            crate::application::performance::record_snapshot(
                snapshot.revision.get(),
                snapshot.messages.len(),
                snapshot_committed_steps,
                snapshot_shared_messages,
                snapshot_started.elapsed(),
            );
        }
        #[cfg(test)]
        let messages_started = std::time::Instant::now();
        let mut messages = snapshot
            .messages
            .with_pending(request.pending_messages.clone());
        let reminder_payloads = invocation_reminder_log_payloads(
            request.language.as_str(),
            &request.invocation_reminders,
        );
        if !reminder_payloads.is_empty() {
            let kinds = reminder_payloads
                .iter()
                .map(|payload| payload.kind)
                .collect::<Vec<_>>()
                .join(",");
            log::debug!(
                target: crate::LOG_TARGET,
                "invocation_reminders_rendered count={} kinds={} request_id={}",
                reminder_payloads.len(),
                kinds,
                request.request_id.as_str(),
            );
            for (placement, payload) in reminder_payloads.iter().enumerate() {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "invocation_reminder_placed kind={} placement={} preview={}",
                    payload.kind,
                    placement,
                    payload.preview,
                );
                log::trace!(
                    target: crate::LOG_TARGET,
                    "invocation_reminder_body kind={} body={}",
                    payload.kind,
                    payload.body,
                );
            }
            messages = messages.with_pending(
                reminder_payloads
                    .into_iter()
                    .map(|payload| share::message::Message::user(payload.rendered_body))
                    .collect(),
            );
        }
        #[cfg(test)]
        let messages_assembly_duration = messages_started.elapsed();

        #[cfg(test)]
        let prompt_started = std::time::Instant::now();
        let prompt = self
            .prompt
            .materialize(request)
            .await
            .map_err(ContextPortError::PromptMaterialization)?;
        #[cfg(test)]
        crate::application::performance::record_prompt(prompt_started.elapsed());
        #[cfg(test)]
        let memory_started = std::time::Instant::now();
        let memory = self
            .memory
            .materialize(request)
            .await
            .map_err(ContextPortError::MemoryMaterialization)?;
        #[cfg(test)]
        crate::application::performance::record_memory(memory_started.elapsed());

        #[cfg(test)]
        let blocks_started = std::time::Instant::now();
        let mut blocks = prompt.cacheable;
        blocks.extend(memory.blocks);
        if let Some(summary) = snapshot.active_summary {
            // #1486 系统护栏：任何来源的 active_summary 注入 system 前都必须
            // 有界。历史缺陷曾让 summary 膨胀到 92 万字符撑爆 system prompt；
            // 此处按预算校验，超限只保留关键尾部并告警。
            let budget = crate::domain::token_budget::summary_budget(request.context_size);
            let summary = if crate::domain::token_budget::estimate_tokens(&summary) > budget {
                let tail = share::string_idx::slice_tail(
                    &summary,
                    crate::domain::token_budget::FALLBACK_PREVIOUS_SUMMARY_CAP,
                )
                .to_string();
                log::warn!(
                    target: crate::LOG_TARGET,
                    "active_summary 超出预算（{} tokens > budget {budget}），截断为 {} chars 尾部",
                    crate::domain::token_budget::estimate_tokens(&summary),
                    tail.len(),
                );
                tail
            } else {
                summary
            };
            blocks.push(SystemBlock {
                kind: "active_summary".into(),
                content: summary,
                cacheable: true,
                cache_break: false,
            });
        }
        if let Some(last_cacheable) = blocks.last_mut() {
            last_cacheable.cache_break = true;
        }
        blocks.extend(prompt.uncached);

        #[cfg(test)]
        {
            let (tool_result_blocks, tool_result_content_bytes) =
                context_message_tool_result_metrics(&messages);
            crate::application::performance::record_assembly(
                crate::application::performance::AssemblyMetrics {
                    pending_messages: request.pending_messages.len(),
                    final_messages: messages.len(),
                    system_blocks: blocks.len(),
                    tool_result_blocks,
                    tool_result_content_bytes,
                },
                messages_assembly_duration.saturating_add(blocks_started.elapsed()),
            );
        }
        #[cfg(test)]
        let decision_started = std::time::Instant::now();
        let token_estimation =
            crate::domain::context_decision::token_budget(request, &messages, &blocks);
        let decision = crate::domain::context_decision::calculate(request, &messages, &blocks);
        #[cfg(test)]
        crate::application::performance::record_decision(
            token_estimation.total_tokens,
            request.last_api_total_tokens,
            decision.decision_token_count,
            decision.reason,
            decision_started.elapsed(),
        );
        let window = ContextWindow {
            backing_revision: snapshot.revision,
            system_blocks: blocks,
            messages,
            tool_schemas: request.tool_schemas.clone(),
            token_estimation,
            compaction_decision: decision,
        };
        #[cfg(test)]
        crate::application::performance::record_build(build_started.elapsed());
        Ok(window)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReminderLogPayload {
    pub kind: &'static str,
    pub preview: String,
    pub body: String,
    pub(crate) rendered_body: String,
}

pub(crate) fn invocation_reminder_log_payloads(
    language: &str,
    reminders: &[InvocationReminder],
) -> Vec<ReminderLogPayload> {
    let mut rendered = Vec::new();
    for reminder_kind in [0_u8, 1, 2] {
        for reminder in reminders {
            let text = match (reminder_kind, reminder) {
                (0, InvocationReminder::TaskProgress(progress)) => {
                    let mut lines = vec![match language {
                        "zh" => format!("━━ 任务：{}/{} ━━", progress.completed, progress.total),
                        _ => format!("━━ Tasks: {}/{} ━━", progress.completed, progress.total),
                    }];
                    for item in &progress.items {
                        let status = match item.status {
                            TaskProgressStatus::Completed => "✓",
                            TaskProgressStatus::InProgress => "■",
                            TaskProgressStatus::Pending => "□",
                        };
                        let blocked = if item.blocked_by_sequences.is_empty() {
                            String::new()
                        } else {
                            let sequences = item
                                .blocked_by_sequences
                                .iter()
                                .map(u64::to_string)
                                .collect::<Vec<_>>()
                                .join(", ");
                            match language {
                                "zh" => format!("（被 #{sequences} 阻塞）"),
                                _ => format!(" (blocked by #{sequences})"),
                            }
                        };
                        lines.push(format!(
                            "{status} #{} {}{blocked}",
                            item.sequence,
                            escape_reminder_text(&item.subject)
                        ));
                    }
                    if progress.hidden_count > 0 {
                        lines.push(match language {
                            "zh" => format!("另有 {} 个任务未显示", progress.hidden_count),
                            _ => format!("{} additional tasks are omitted", progress.hidden_count),
                        });
                    }
                    let heading = match language {
                        "zh" => "当前任务进度：",
                        _ => "Current task progress:",
                    };
                    Some(format!(
                        "<system-reminder>{heading}\n{}\n</system-reminder>",
                        lines.join("\n")
                    ))
                }
                (1, InvocationReminder::GuidanceSourcesChanged) => {
                    Some(match language {
                        "zh" => "<system-reminder>guidance 来源已变更；当前 Session 的冻结系统提示保持不变。新 Session 才会重新物化这些来源。</system-reminder>".to_string(),
                        _ => "<system-reminder>Guidance sources changed. This Session's frozen system prompt remains unchanged; a new Session will materialize the updated sources.</system-reminder>".to_string(),
                    })
                }
                (
                    2,
                    InvocationReminder::ModelGuidanceMismatch {
                        session_model_id,
                        run_model_id,
                    },
                ) => Some(match language {
                    "zh" => format!(
                        "<system-reminder>Session 冻结模型 {} 与当前 Run 模型 {} 不同；继续使用 Session 冻结的系统提示。</system-reminder>",
                        escape_reminder_text(session_model_id),
                        escape_reminder_text(run_model_id)
                    ),
                    _ => format!(
                        "<system-reminder>The Session-frozen model {} differs from the current Run model {}; continue using the Session-frozen system prompt.</system-reminder>",
                        escape_reminder_text(session_model_id),
                        escape_reminder_text(run_model_id)
                    ),
                }),
                _ => None,
            };
            if let Some(text) = text {
                let body = redact_reminder_log_text(&text);
                rendered.push(ReminderLogPayload {
                    kind: reminder.kind(),
                    preview: reminder_log_preview(&body),
                    body,
                    rendered_body: text,
                });
            }
        }
    }
    rendered
}

fn reminder_log_preview(body: &str) -> String {
    let mut preview = body.chars().take(200).collect::<String>();
    if body.chars().count() > 200 {
        preview.push('…');
    }
    preview
}

fn redact_reminder_log_text(text: &str) -> String {
    let words = text.split_whitespace().collect::<Vec<_>>();
    let mut redacted = Vec::with_capacity(words.len());
    let mut redact_next = false;
    for word in words {
        let normalized = word
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '-')
            .to_ascii_lowercase();
        if redact_next {
            if normalized == "bearer" {
                redacted.push(word);
                continue;
            }
            redacted.push("[REDACTED]");
            redact_next = false;
            continue;
        }
        if looks_like_secret(&normalized) {
            redacted.push("[REDACTED]");
            continue;
        }
        redacted.push(word);
        redact_next = matches!(
            normalized.as_str(),
            "authorization" | "api_key" | "api-key" | "token" | "secret"
        );
    }
    redacted.join(" ")
}

fn looks_like_secret(normalized: &str) -> bool {
    normalized.starts_with("sk-")
        || normalized.starts_with("ghp_")
        || normalized.starts_with("github_pat_")
}

fn escape_reminder_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
fn context_message_tool_result_metrics(messages: &crate::domain::ContextMessages) -> (usize, u64) {
    messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|block| match block {
            share::message::ContentBlock::ToolResult { content, .. } => Some(content),
            _ => None,
        })
        .fold((0usize, 0u64), |(count, bytes), content| {
            (
                count.saturating_add(1),
                bytes.saturating_add(u64::try_from(content.to_string().len()).unwrap_or(u64::MAX)),
            )
        })
}

#[async_trait]
impl ContextPort for ContextApplicationService {
    async fn build_window(
        &self,
        request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError> {
        self.build_candidate(request).await
    }

    async fn needs_compaction(
        &self,
        request: &ContextRequest,
    ) -> Result<CompactionDecision, ContextPortError> {
        Ok(self.build_candidate(request).await?.compaction_decision)
    }

    async fn compact(&self, request: &CompactRequest) -> Result<CompactOutcome, ContextPortError> {
        self.session.commit_compaction(request).await
    }

    async fn manual_compact(
        &self,
        request: &ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        self.session.commit_manual_compaction(request).await
    }

    async fn clear_session(&self, session_id: &SessionId) -> Result<(), ContextPortError> {
        self.session.clear(session_id).await
    }

    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppend,
    ) -> Result<AcceptedInputReceipt, AcceptedInputError> {
        self.session.append_accepted_input(append).await
    }

    async fn advance_tool_receipt(
        &self,
        mutation: ToolReceiptMutation,
    ) -> Result<ToolReceiptMutationReceipt, ToolReceiptMutationError> {
        self.session.advance_tool_receipt(mutation).await
    }

    async fn step_receipts(
        &self,
        session_id: &SessionId,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
    ) -> Result<Vec<crate::domain::StepReceipt>, ToolReceiptMutationError> {
        self.session
            .step_receipts(session_id, run_id, step_id)
            .await
    }

    async fn compare_and_record_skill_load(
        &self,
        mutation: tools::SkillLoadMutation,
    ) -> Result<tools::SkillLoadDecision, tools::SkillLoadStateError> {
        self.session.compare_and_record_skill_load(mutation).await
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError> {
        self.session.append_finalized(append).await
    }
}
