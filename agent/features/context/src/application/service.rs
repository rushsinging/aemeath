use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::application::reminder_pipeline::ReminderPipeline;
use crate::domain::reminder::{ReminderEventSource, ReminderSource};
use crate::domain::{
    AcceptedInputAppendData, AcceptedInputError, AcceptedInputReceiptData, AppendReceiptData,
    CompactOutcome, CompactRequestData, CompactionDecisionData, ContextAppendData,
    ContextAppendError, ContextMessage, ContextPortError, ContextRequestData, ContextWindowData,
    ManualCompactRequestData, RunId, SessionId, SystemBlock, ToolReceiptMutationData,
    ToolReceiptMutationError, ToolReceiptMutationReceiptData,
};
use crate::ports::{ContextMemorySource, ContextPort, ContextPromptSource, SessionRepository};

pub(crate) struct ContextApplicationService {
    session: Arc<dyn SessionRepository>,
    prompt: Arc<dyn ContextPromptSource>,
    memory: Arc<dyn ContextMemorySource>,
    /// 本 Session 冻结的注入内容（#1777）。记忆块属于可缓存 system
    /// prefix：每轮重新检索会让内容漂移，直接损害 provider 的 prompt cache
    /// 命中率。刷新点只有「Session 首次」与「compact 成功后」。
    frozen_injection: std::sync::Arc<std::sync::Mutex<Option<FrozenInjection>>>,
    /// compact 成功后置位：下一轮 build window 重新检索并替换冻结内容。
    injection_refresh_pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Run-scoped reminder 管线：Run 启动创建、结束销毁（07-reminder-pipeline.md）。
    reminder_pipelines:
        std::sync::Arc<std::sync::Mutex<HashMap<crate::domain::RunId, ReminderPipeline>>>,
}

/// 注入冻结状态。绑定 `session_id`：resume 切换到新 Session 视为该 Session
/// 的首次注入。
#[derive(Debug, Clone)]
struct FrozenInjection {
    session_id: SessionId,
    materialization: crate::ports::MemoryMaterialization,
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
            frozen_injection: std::sync::Arc::new(std::sync::Mutex::new(None)),
            injection_refresh_pending: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
            reminder_pipelines: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// 取本轮的注入内容：命中冻结则复用，否则检索并冻结。
    async fn injection_for(
        &self,
        request: &ContextRequestData,
    ) -> Result<crate::ports::MemoryMaterialization, ContextPortError> {
        let refresh = self
            .injection_refresh_pending
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        if !refresh {
            if let Some(frozen) = self
                .frozen_injection
                .lock()
                .expect("injection lock poisoned")
                .as_ref()
                .filter(|frozen| frozen.session_id == request.session_id)
            {
                return Ok(frozen.materialization.clone());
            }
        }
        let materialization = self
            .memory
            .materialize(request)
            .await
            .map_err(ContextPortError::MemoryMaterialization)?;
        *self
            .frozen_injection
            .lock()
            .expect("injection lock poisoned") = Some(FrozenInjection {
            session_id: request.session_id.clone(),
            materialization: materialization.clone(),
        });
        Ok(materialization)
    }

    /// compact 改写了对话历史，冻结的注入内容随之过期。
    /// 取走本 Run 待落盘的注入 reminder 消息（无管线返回空）。
    fn take_pending_reminder_messages(&self, run_id: &RunId) -> Vec<ContextMessage> {
        self.reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned")
            .get_mut(run_id)
            .map(|pipeline| pipeline.take_pending_persist_messages())
            .unwrap_or_default()
    }

    fn mark_injection_stale(&self) {
        self.injection_refresh_pending
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// compact `Committed` 后的 reminder 处置（run_id 来自 CompactRequestData）。
    fn reminder_compact_committed(&self, run_id: &RunId) {
        if let Some(pipeline) = self
            .reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned")
            .get_mut(run_id)
        {
            pipeline.compact_committed();
        }
    }

    /// build_window 的 reminder 注入：无管线（W2 迁移期）返回空产物。
    fn reminder_injection_for(
        &self,
        request: &ContextRequestData,
    ) -> crate::application::reminder_pipeline::ReminderWindowInjection {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut pipelines = self
            .reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned");
        match pipelines.get_mut(&request.run_id) {
            Some(pipeline) => pipeline.inject_into_window(
                request.language.as_str(),
                &now,
                crate::domain::REMINDER_INJECTION_TOKEN_BUDGET,
            ),
            None => crate::application::reminder_pipeline::ReminderWindowInjection::default(),
        }
    }

    async fn build_candidate(
        &self,
        request: &ContextRequestData,
    ) -> Result<ContextWindowData, ContextPortError> {
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
        let committed_messages = if let Some(history) = snapshot.structured_history.as_ref() {
            let candidate = crate::domain::compact::ContextReadCandidate::from_history(
                history,
                request.run_id.as_ref(),
                crate::domain::compact::ProtectedRunPolicy::latest_complete_runs(3),
            );
            let candidate = if request.config_snapshot.context_snip_enabled() {
                crate::domain::compact::snip_superseded_exploration(&candidate)
            } else {
                candidate
            };
            let candidate = if request.config_snapshot.context_microcompact_enabled() {
                crate::domain::compact::microcompact_exploration(&candidate)
            } else {
                candidate
            };
            candidate.messages()
        } else {
            snapshot.messages.clone()
        };
        let mut messages = committed_messages.with_pending(request.pending_messages.clone());
        // Reminder 统一管线注入（07-reminder-pipeline.md）：Run-scoped
        // 管线按 policy 注入（placement / dedup / 预算 / 拼装）。
        let reminder_injection = self.reminder_injection_for(request);
        if let Some(tail_message) = reminder_injection.tail_user_message.clone() {
            messages = messages.with_pending(vec![share::message::Message::user(tail_message)]);
        }
        // LLM 视图收口（specs/3.7 §18）：为带输入时刻的 user 消息渲染时间前缀；
        // canonical 与持久化 JSON 不含前缀，无 created_at 的消息原样保留。
        let messages = messages.map_messages(render_user_input_timestamp_prefix);
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
        let memory = self.injection_for(request).await?;
        #[cfg(test)]
        crate::application::performance::record_memory(memory_started.elapsed());

        #[cfg(test)]
        let blocks_started = std::time::Instant::now();
        let mut blocks = prompt.cacheable;
        blocks.extend(memory.blocks);
        blocks.extend(reminder_injection.system_tail_blocks);
        if let Some(summary) = snapshot.active_summary {
            let budget = crate::domain::token_budget::summary_budget(request.context_size);
            let estimated_tokens = crate::domain::token_budget::estimate_tokens(&summary);
            let summary = if estimated_tokens > budget {
                let decoded = crate::domain::compact::CanonicalCompactSummary::decode(&summary)
                    .map_err(|error| {
                        ContextPortError::Compact(format!(
                            "active_summary 超出预算且无法解析为 canonical checkpoint：{error}"
                        ))
                    })?;
                let task_state_companion = decoded.task_state_companion().map(str::to_string);
                let bounded_checkpoint = decoded
                    .into_checkpoint()
                    .degrade_to_budget(budget)
                    .map_err(|error| {
                        ContextPortError::Compact(format!(
                            "active_summary 超出预算且无法安全降级：{error}"
                        ))
                    })?;
                let bounded_summary = match task_state_companion {
                    Some(companion) => format!(
                        "{}{}{companion}",
                        bounded_checkpoint.render(),
                        crate::domain::compact::TASK_STATE_HEADING
                    ),
                    None => bounded_checkpoint.render(),
                };
                let bounded_tokens = crate::domain::token_budget::estimate_tokens(&bounded_summary);
                if bounded_tokens > budget {
                    return Err(ContextPortError::Compact(format!(
                        "active_summary 结构化降级后仍超出预算：{bounded_tokens} tokens > budget {budget}"
                    )));
                }
                log::warn!(
                    target: crate::LOG_TARGET,
                    "active_summary 超出预算，按 checkpoint 语义降级：{estimated_tokens} -> {bounded_tokens} tokens（预算 {budget}）",
                );
                bounded_summary
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
        let window = ContextWindowData {
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

    /// compact 提交后的占用体检报告（防震荡观测信号）。
    pub(crate) async fn post_compaction_usage_check(
        &self,
        source: &ContextRequestData,
    ) -> Option<PostCompactionUsageReport> {
        // compact 刚重置 usage baseline，source 里携带的 provider 旧值
        // 不代表提交后的状态，强制走 heuristic 估算路径。
        let mut source = source.clone();
        source.last_api_total_tokens = None;
        match self.build_candidate(&source).await {
            Ok(candidate) => Some(PostCompactionUsageReport {
                decision_token_count: candidate.compaction_decision.decision_token_count,
                threshold: candidate.compaction_decision.threshold,
            }),
            Err(error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "compact 后占用体检失败（不阻断）：{error}"
                );
                None
            }
        }
    }
}

/// compact 提交后的占用体检结果。
///
/// 估算仍高于 threshold 的 50% 视为贴线：保留消息 + system/tool schema
/// 固定底盘几轮后将再次触发 auto-compact（震荡循环）。体检只产出
/// 报告与告警，不阻断 compact、不自动二次压缩。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PostCompactionUsageReport {
    decision_token_count: usize,
    threshold: usize,
}

impl PostCompactionUsageReport {
    pub fn decision_token_count(&self) -> usize {
        self.decision_token_count
    }

    pub fn threshold(&self) -> usize {
        self.threshold
    }

    /// 估算是否仍高于 threshold 的一半（贴线，震荡风险）。
    pub fn exceeds_half_threshold(&self) -> bool {
        self.decision_token_count > self.threshold / 2
    }
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
        request: &ContextRequestData,
    ) -> Result<ContextWindowData, ContextPortError> {
        self.build_candidate(request).await
    }

    async fn needs_compaction(
        &self,
        request: &ContextRequestData,
    ) -> Result<CompactionDecisionData, ContextPortError> {
        Ok(self.build_candidate(request).await?.compaction_decision)
    }

    async fn compact(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        let outcome = self.session.commit_compaction(request).await?;
        // 仅在真实提交后体检：Skipped 时会话状态未变，重建无意义。
        if matches!(outcome, CompactOutcome::Committed(_)) {
            // #1777：compact 重写了对话，冻结的注入内容随之过期。
            self.mark_injection_stale();
            // Reminder 统一管线：按 per-kind compact 处置重开（07-reminder-pipeline.md）。
            self.reminder_compact_committed(&request.run_id);
            if let Some(report) = self.post_compaction_usage_check(&request.source).await {
                if report.exceeds_half_threshold() {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "compact 提交后估算仍贴线（{} > threshold/2 = {}）：保留消息与固定底盘几轮后将再次触发 auto-compact；若频繁出现，考虑增大 context window 或调小保留窗口",
                        report.decision_token_count(),
                        report.threshold() / 2
                    );
                }
            }
        }
        Ok(outcome)
    }

    async fn manual_compact(
        &self,
        request: &ManualCompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        let outcome = self.session.commit_manual_compaction(request).await?;
        if matches!(outcome, CompactOutcome::Committed(_)) {
            // #1777：手动 compact 同样刷新冻结的注入内容。
            self.mark_injection_stale();
        }
        Ok(outcome)
    }

    async fn clear_session(&self, session_id: &SessionId) -> Result<(), ContextPortError> {
        let cleared = self.session.clear(session_id).await?;
        // 清除的是该 Session 的冻结注入，下一次窗口重新注入。
        let mut frozen = self
            .frozen_injection
            .lock()
            .expect("injection lock poisoned");
        if frozen
            .as_ref()
            .is_some_and(|held| &held.session_id == session_id)
        {
            *frozen = None;
        }
        Ok(cleared)
    }

    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppendData,
    ) -> Result<AcceptedInputReceiptData, AcceptedInputError> {
        self.session.append_accepted_input(append).await
    }

    async fn advance_tool_receipt(
        &self,
        mutation: ToolReceiptMutationData,
    ) -> Result<ToolReceiptMutationReceiptData, ToolReceiptMutationError> {
        self.session.advance_tool_receipt(mutation).await
    }

    async fn step_receipts(
        &self,
        session_id: &SessionId,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
    ) -> Result<Vec<crate::domain::StepReceiptData>, ToolReceiptMutationError> {
        self.session
            .step_receipts(session_id, run_id, step_id)
            .await
    }

    async fn compare_and_record_skill_load(
        &self,
        mutation: tools::published::skill::SkillLoadMutation,
    ) -> Result<
        tools::published::skill::SkillLoadDecision,
        tools::published::skill::SkillLoadStateError,
    > {
        self.session.compare_and_record_skill_load(mutation).await
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppendData,
    ) -> Result<AppendReceiptData, ContextAppendError> {
        // #1848 尾部注入类 reminder 显式落盘：flush 本 Run 待落盘的注入
        // 消息并前置到提交消息头部（canonical 顺序 u → R → a/tool），
        // 使注入轮后的请求前缀逐 token 稳定（prompt cache 命中）。
        // runtime 调用零改动；fingerprint 保留调用方原值——重试路径
        //（pending 已 flush）幂等早退不重复插入。
        let pending_reminders = self.take_pending_reminder_messages(&append.run_id);
        if pending_reminders.is_empty() {
            return self.session.append_finalized(append).await;
        }
        let mut enriched = append.clone();
        let mut messages = pending_reminders;
        messages.extend(append.messages.iter().cloned());
        enriched.messages = messages;
        self.session.append_finalized(&enriched).await
    }

    // ─── Reminder 统一管线控制面（ContextPort 默认方法的生产实现）───

    /// Reminder 管线句柄：Run 启动创建（同 RunId 重复创建替换旧管线）。
    fn create_reminder_pipeline(&self, run_id: RunId, sources: Vec<Arc<dyn ReminderSource>>) {
        self.reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned")
            .insert(run_id, ReminderPipeline::new(sources));
    }

    /// Reminder 管线句柄：Run 结束销毁。
    fn drop_reminder_pipeline(&self, run_id: &RunId) {
        self.reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned")
            .remove(run_id);
    }

    /// Run 启动事件：OnRunStart 类 source 入队。
    fn reminder_run_started(&self, run_id: &RunId) {
        if let Some(pipeline) = self
            .reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned")
            .get_mut(run_id)
        {
            pipeline.run_started();
        }
    }

    /// Runtime 推送 reminder 事件：OnEvent(source) 匹配的 source 入队。
    fn reminder_handle_event(&self, run_id: &RunId, event_source: &ReminderEventSource) {
        if let Some(pipeline) = self
            .reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned")
            .get_mut(run_id)
        {
            pipeline.handle_event(event_source);
        }
    }

    /// task store 变更事件：OnTaskMutation 类 source 入队。
    fn reminder_task_mutated(&self, run_id: &RunId) {
        if let Some(pipeline) = self
            .reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned")
            .get_mut(run_id)
        {
            pipeline.task_mutated();
        }
    }

    /// step 边界事件：OnStepInterval(n) 在步数为 n 的倍数时重建入队。
    fn reminder_step_advanced(&self, run_id: &RunId, step: u64) {
        if let Some(pipeline) = self
            .reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned")
            .get_mut(run_id)
        {
            pipeline.step_advanced(step);
        }
    }
    fn reminder_user_message_received(&self, run_id: &RunId) {
        if let Some(pipeline) = self
            .reminder_pipelines
            .lock()
            .expect("reminder pipelines lock poisoned")
            .get_mut(run_id)
        {
            pipeline.user_message_received();
        }
    }
}

/// 为带用户输入时刻的 user 消息渲染 LLM 时间前缀 `[YYYY-MM-DD HH:MM ±ZZZZ] `，
/// 仅作用于 ContextWindow 视图（canonical message 与落盘 JSON 不变）。
/// 返回 `None` 表示原样保留：非 user、无 `created_at`（系统生成 / tool result /
/// reminder）或无 Text block 的消息都不加前缀。
pub(crate) fn render_user_input_timestamp_prefix(
    message: &share::message::Message,
) -> Option<share::message::Message> {
    if message.role != share::message::Role::User {
        return None;
    }
    let created_at = message.metadata.as_ref()?.created_at?;
    let prefix = format!("[{}]: ", created_at.format("%Y-%m-%d %H:%M %z"));
    let mut rendered = message.clone();
    let first_text = rendered.content.iter_mut().find_map(|block| match block {
        share::message::ContentBlock::Text { text } => Some(text),
        _ => None,
    })?;
    *first_text = format!("{prefix}{first_text}");
    Some(rendered)
}
