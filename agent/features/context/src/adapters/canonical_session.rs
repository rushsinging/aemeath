use share::ids::SessionId;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::adapters::compact_summary::compact_messages_with_llm;
use crate::domain::session::{
    AcceptedInputRecord, ActiveCompactMarker, CanonicalSession, CommittedStep,
    FinalizedOutcomeRecord, SessionCommitPlan, SessionSnapshotState,
};
use crate::domain::{
    AcceptedInputAppendData, AcceptedInputError, CompactOutcome, CompactRequestData,
    CompactSkipReason, CompactTrigger, ContextAppendData, ContextAppendError, ContextPortError,
    SessionAppendReceiptData, SessionRevision, ToolReceiptMutationData, ToolReceiptMutationError,
    ToolReceiptMutationReceiptData,
};
use crate::ports::{ContextPorts, SessionItem, SessionRepository};

#[async_trait]
pub trait CanonicalSessionWriter: Send + Sync {
    async fn commit(
        &self,
        session_id: &str,
        expected_revision: u64,
        plan: SessionCommitPlan,
    ) -> Result<(), String>;

    /// `/clear` 逻辑断点提交：磁盘 step 成员全部保留，state 记录 clear
    /// 边界（clear 时刻磁盘 generation 的最后一个 step）。
    ///
    /// 返回写入 clear 边界后的 session 快照（与磁盘一致），调用方必须以
    /// 返回值发布内存 generation。默认实现面向无磁盘增量的 backing：
    /// 无未加载成员需要保留，边界保持 `after` 原样。
    async fn commit_clearing_history(
        &self,
        before: &CanonicalSession,
        after: CanonicalSession,
    ) -> Result<CanonicalSession, String> {
        let plan = SessionCommitPlan::between(before, &after).map_err(|error| error.to_string())?;
        self.commit(after.id.as_str(), before.revision, plan)
            .await?;
        Ok(after)
    }

    /// 数据集为空集时以内存全量快照重建持久化状态；数据集非空时必须
    /// fail-closed，防止覆盖并发写者。重建后磁盘修订号与内存对齐。
    async fn rebuild_empty_dataset(
        &self,
        _session_id: &str,
        _session: &crate::domain::session::CanonicalSession,
    ) -> Result<(), String> {
        Err("Session writer 不支持空数据集全量重建".to_string())
    }
}

pub struct NoOpCanonicalSessionWriter;

#[async_trait]
impl CanonicalSessionWriter for NoOpCanonicalSessionWriter {
    async fn commit(
        &self,
        _session_id: &str,
        _expected_revision: u64,
        _plan: SessionCommitPlan,
    ) -> Result<(), String> {
        Ok(())
    }
}

pub struct MainContextFactory {
    writer: Arc<dyn CanonicalSessionWriter>,
    task_persist: Option<Arc<dyn task::TaskPersist>>,
    workspace_persist: Option<Arc<dyn project::WorkspaceWriter>>,
    /// 可选注入的 Skill metadata catalog 与 Context-owned query factory。
    skill_catalog: Option<Arc<dyn tools::published::skill::SkillCatalogPort>>,
    query_factory: Option<Arc<dyn crate::ports::SkillQueryFactory>>,
    /// Hook BC 注入的指令加载通知端口：Context 组装系统提示时触发。
    instructions_loaded: Option<Arc<dyn share::instructions::InstructionsLoadedNotifier>>,
}

impl MainContextFactory {
    pub fn new(writer: Arc<dyn CanonicalSessionWriter>) -> Self {
        Self {
            task_persist: None,
            workspace_persist: None,
            writer,
            skill_catalog: None,
            query_factory: None,
            instructions_loaded: None,
        }
    }

    /// 注入指令加载通知端口（hook 域定义，Runtime 装配实现）。
    pub fn with_instructions_loaded(
        mut self,
        notifier: Arc<dyn share::instructions::InstructionsLoadedNotifier>,
    ) -> Self {
        self.instructions_loaded = Some(notifier);
        self
    }

    pub fn with_skill_catalog(
        mut self,
        catalog: Arc<dyn tools::published::skill::SkillCatalogPort>,
        query_factory: Arc<dyn crate::ports::SkillQueryFactory>,
    ) -> Self {
        self.skill_catalog = Some(catalog);
        self.query_factory = Some(query_factory);
        self
    }

    /// 注入稳定持久化 port（task / workspace）；build 时不再重复传递。
    pub fn with_persistence(
        mut self,
        task_persist: Arc<dyn task::TaskPersist>,
        workspace_persist: Arc<dyn project::WorkspaceWriter>,
    ) -> Self {
        self.task_persist = Some(task_persist);
        self.workspace_persist = Some(workspace_persist);
        self
    }
}

impl MainContextFactory {
    /// 构建 per-session 活状态的 ContextPorts 聚合。
    pub fn build(
        &self,
        session: Arc<RwLock<Arc<CanonicalSession>>>,
        memory: Arc<RwLock<Arc<dyn memory::api::MemoryPort>>>,
        mutation_gate: Arc<tokio::sync::Mutex<()>>,
    ) -> ContextPorts {
        let task_persist = self
            .task_persist
            .clone()
            .expect("factory task persistence must be injected via with_persistence");
        let workspace_persist = self
            .workspace_persist
            .clone()
            .expect("factory workspace persistence must be injected via with_persistence");
        let assembly = Arc::new(crate::adapters::SystemPromptAssembly::new(
            self.instructions_loaded.clone(),
        ));
        let prompt: Arc<dyn crate::ports::ContextPromptSource> =
            match (&self.skill_catalog, &self.query_factory) {
                (Some(catalog), Some(factory)) => {
                    Arc::new(crate::adapters::SkillPromptSource::new(
                        Arc::clone(catalog),
                        Arc::clone(factory),
                        Arc::clone(&assembly),
                    ))
                }
                _ => Arc::new(crate::adapters::BaselinePromptSource::new(assembly)),
            };
        let repository = CanonicalSessionRepository::new(
            session,
            task_persist,
            workspace_persist,
            Arc::clone(&self.writer),
            mutation_gate,
        );
        ContextPorts::from_service(Arc::new(
            crate::application::ContextApplicationService::new(
                Arc::new(repository),
                prompt,
                Arc::new(crate::adapters::CommittedMemoryRetrieveAdapter::new(memory)),
            ),
        ))
    }
}

#[derive(Clone)]
struct CompactSource {
    revision: SessionRevision,
    messages: Vec<share::message::Message>,
    visible_steps: Vec<(
        crate::domain::session::RunStepCursor,
        Vec<share::message::Message>,
    )>,
    /// 各 Step 在 `messages` 扁平序列中的起始索引（升序），供 #1688
    /// tail token 封顶软对齐使用；来源为 canonical visible_steps。
    step_boundaries: Vec<usize>,
    previous_summary: Option<String>,
}

struct GeneratedCompact {
    summary: String,
    recent_messages: Vec<share::message::Message>,
    quality: crate::domain::CompactSummaryQuality,
    metrics: crate::domain::compact::CompactMetricsData,
}

struct AutoCompactAttemptPermit {
    state: Arc<std::sync::Mutex<crate::domain::compact::AutoCompactState>>,
    failure_limit: u8,
    finished: bool,
}

impl AutoCompactAttemptPermit {
    fn finish(mut self, outcome: &Result<CompactOutcome, ContextPortError>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match outcome {
            Ok(CompactOutcome::Committed(_)) => state.record_success(),
            Ok(CompactOutcome::Skipped(_)) => {}
            Err(_) => state.record_failure(self.failure_limit),
        }
        self.finished = true;
    }
}

impl Drop for AutoCompactAttemptPermit {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .record_failure(self.failure_limit);
    }
}

pub(crate) struct CanonicalSessionRepository {
    session: Arc<RwLock<Arc<CanonicalSession>>>,
    task_persist: Arc<dyn task::TaskPersist>,
    workspace_persist: Arc<dyn project::WorkspaceWriter>,
    writer: Arc<dyn CanonicalSessionWriter>,
    mutation_gate: Arc<tokio::sync::Mutex<()>>,
    /// 可选注入的 LLM 摘要生成器（#1486）。Some 时 compact 走 LLM 语义压缩，
    /// 失败自动 fallback 本地；None 时直接走本地文本压缩。
    auto_compact_state: Arc<std::sync::Mutex<crate::domain::compact::AutoCompactState>>,
}

impl CanonicalSessionRepository {
    pub fn new(
        session: Arc<RwLock<Arc<CanonicalSession>>>,
        task_persist: Arc<dyn task::TaskPersist>,
        workspace_persist: Arc<dyn project::WorkspaceWriter>,
        writer: Arc<dyn CanonicalSessionWriter>,
        mutation_gate: Arc<tokio::sync::Mutex<()>>,
    ) -> Self {
        Self {
            session,
            task_persist,
            workspace_persist,
            writer,
            mutation_gate,
            auto_compact_state: Arc::new(std::sync::Mutex::new(
                crate::domain::compact::AutoCompactState::default(),
            )),
        }
    }

    /// 压缩可见消息：
    /// - 注入 generator 时走 LLM 语义压缩（`compact_messages_with_llm`，
    ///   内部失败自动 fallback 本地，分块并发 + 汇总收敛）；
    /// - 未注入时走本地文本压缩 `compact_messages`，并手动补齐
    ///   previous_summary（`build_summary_text` 已保证不全文累加）。
    ///
    /// `progress` 为压缩进度回调（#1500）：Preparing/Summarizing chunk
    /// 计数/Finalizing 阶段实时上报；`None` 表示调用方不关心进度。
    ///
    /// 返回 `(summary, recent_messages)`；消息太少无法压缩时返回 `None`。
    #[allow(clippy::too_many_arguments)]
    async fn compact_visible_messages(
        &self,
        messages: &[share::message::Message],
        previous_summary: Option<&str>,
        context_size: usize,
        step_boundaries: &[usize],
        progress: Option<std::sync::Arc<dyn crate::domain::CompactProgressFn>>,
        task_snapshot: Option<&task::CompactTaskSnapshotData>,
        llm: Option<&crate::domain::CompactLlmBinding>,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Option<crate::adapters::compact_summary::CompactResult> {
        let tail = crate::adapters::compact_summary::CompactTail::from_context(
            step_boundaries,
            context_size,
        );
        match llm {
            Some(binding) => {
                let result = compact_messages_with_llm(
                    messages,
                    previous_summary,
                    context_size,
                    Some(binding),
                    progress.as_deref(),
                    task_snapshot,
                    cancellation,
                    tail,
                )
                .await;
                result.inspect(|compacted| {
                    log::info!(
                        target: crate::LOG_TARGET,
                        "[compact] LLM 路径生成完成：summary={} chars recent={} quality={:?}",
                        compacted.summary.len(),
                        compacted.recent_messages.len(),
                        compacted.quality,
                    );
                })
            }
            None => {
                let mut compacted =
                    crate::adapters::compact_summary::compact_messages(messages, tail)?;
                let window = crate::adapters::compact_summary::compact_window_with_budget(
                    messages,
                    tail.step_boundaries,
                    tail.token_cap,
                )?;
                let early = &messages[..window.split_point]; // allow unsafe_text_op: Vec slice
                compacted.summary =
                    crate::adapters::compact_summary::build_summary_text(early, previous_summary);
                log::info!(
                    target: crate::LOG_TARGET,
                    "[compact] 本地路径生成完成：summary={} chars recent={}",
                    compacted.summary.len(),
                    compacted.recent_messages.len(),
                );
                Some(compacted)
            }
        }
    }

    /// 将 typed TaskData snapshot 确定性渲染为非权威 companion，并将完整 summary
    /// 收敛到同一 Context-owned summary budget。
    fn append_task_snapshot_companion(
        summary: &str,
        task_snapshot: Option<&task::CompactTaskSnapshotData>,
        budget: usize,
    ) -> Result<String, crate::domain::compact::CheckpointError> {
        let checkpoint =
            crate::domain::compact::CanonicalCompactSummary::decode(summary)?.into_checkpoint();
        let Some(snapshot) = task_snapshot.filter(|snapshot| !snapshot.items().is_empty()) else {
            return checkpoint
                .degrade_to_budget(budget)
                .map(|checkpoint| checkpoint.render());
        };

        let mut item_limit = snapshot.items().len();
        loop {
            let companion = snapshot.render_companion_with_limit(item_limit);
            let companion_tokens = crate::domain::token_budget::estimate_tokens(&format!(
                "{}{companion}",
                crate::domain::compact::TASK_STATE_HEADING
            ));
            if companion_tokens < budget {
                if let Ok(bounded_checkpoint) = checkpoint
                    .clone()
                    .degrade_to_budget(budget - companion_tokens)
                {
                    return Ok(format!(
                        "{}{}{companion}",
                        bounded_checkpoint.render(),
                        crate::domain::compact::TASK_STATE_HEADING
                    ));
                }
            }
            if item_limit == 0 {
                break;
            }
            item_limit -= 1;
        }

        checkpoint
            .degrade_to_budget(budget)
            .map(|checkpoint| checkpoint.render())
    }

    async fn commit_automatic_compaction(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        let source = self
            .freeze_compact_source(&request.source.session_id, Some(request.source_revision))
            .await?;
        let generated = match self
            .generate_compact(
                &source,
                request.source.context_size,
                request.progress.clone(),
                request.task_snapshot.as_ref(),
                request.llm.as_ref(),
                &request.cancellation,
            )
            .await
        {
            Ok(Some(generated)) => generated,
            Ok(None) => {
                return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
            }
            Err(reason) => return Ok(CompactOutcome::Skipped(reason)),
        };
        self.commit_generated_compact(&request.source.session_id, &source, generated)
            .await
    }

    fn begin_auto_compact_attempt(&self, failure_limit: u8) -> Option<AutoCompactAttemptPermit> {
        let state = self
            .auto_compact_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.should_attempt() {
            return None;
        }
        drop(state);
        Some(AutoCompactAttemptPermit {
            state: Arc::clone(&self.auto_compact_state),
            failure_limit: failure_limit.max(1),
            finished: false,
        })
    }

    async fn freeze_compact_source(
        &self,
        session_id: &SessionId,
        expected_revision: Option<SessionRevision>,
    ) -> Result<CompactSource, ContextPortError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))?
            .clone();
        if current.id != session_id.as_str() {
            return Err(ContextPortError::SessionNotFound(session_id.clone()));
        }
        let revision = SessionRevision::new(current.revision);
        if let Some(expected_revision) = expected_revision {
            if expected_revision != revision {
                return Err(Self::compact_revision_conflict(expected_revision, revision));
            }
        }
        let visible_steps = current.flattened_steps_from_marker();
        let messages = visible_steps
            .iter()
            .flat_map(|(_, messages)| messages.iter().cloned())
            .collect();
        let step_boundaries = visible_steps
            .iter()
            .scan(0usize, |offset, (_, step_messages)| {
                let boundary = *offset;
                *offset += step_messages.len();
                Some(boundary)
            })
            .collect();
        Ok(CompactSource {
            revision,
            step_boundaries,
            messages,
            visible_steps,
            previous_summary: current
                .compact
                .as_ref()
                .map(|marker| marker.summary.clone()),
        })
    }

    async fn generate_compact(
        &self,
        source: &CompactSource,
        context_size: usize,
        progress: Option<std::sync::Arc<dyn crate::domain::CompactProgressFn>>,
        task_snapshot: Option<&task::CompactTaskSnapshotData>,
        llm: Option<&crate::domain::CompactLlmBinding>,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<Option<GeneratedCompact>, crate::domain::CompactSkipReason> {
        let Some(compacted) = self
            .compact_visible_messages(
                &source.messages,
                source.previous_summary.as_deref(),
                context_size,
                &source.step_boundaries,
                progress,
                task_snapshot,
                llm,
                cancellation,
            )
            .await
        else {
            return if cancellation.is_cancelled() {
                Err(crate::domain::CompactSkipReason::Cancelled)
            } else {
                Ok(None)
            };
        };
        let reconciled_summary =
            crate::domain::compact::CanonicalCompactSummary::decode(&compacted.summary)
                .map(crate::domain::compact::CanonicalCompactSummary::into_checkpoint)
                .and_then(|checkpoint| {
                    crate::domain::compact::reconcile_checkpoint_with_task_snapshot(
                        checkpoint,
                        task_snapshot,
                    )
                })
                .map(|checkpoint| checkpoint.render())
                .unwrap_or_else(|_| compacted.summary.clone());
        let summary = Self::append_task_snapshot_companion(
            &reconciled_summary,
            task_snapshot,
            crate::domain::token_budget::summary_budget(context_size),
        )
        .map_err(|_| crate::domain::CompactSkipReason::CircuitBreakerOpen)?;
        Ok(Some(GeneratedCompact {
            summary,
            recent_messages: compacted.recent_messages,
            quality: compacted.quality,
            metrics: compacted.metrics,
        }))
    }

    async fn commit_generated_compact(
        &self,
        session_id: &SessionId,
        source: &CompactSource,
        generated: GeneratedCompact,
    ) -> Result<CompactOutcome, ContextPortError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))?
            .clone();
        if current.id != session_id.as_str() {
            return Err(ContextPortError::SessionNotFound(session_id.clone()));
        }
        let actual_revision = SessionRevision::new(current.revision);
        if actual_revision != source.revision {
            return Err(Self::compact_revision_conflict(
                source.revision,
                actual_revision,
            ));
        }
        let keep_messages = generated.recent_messages.len();
        let mut retained = 0usize;
        let mut start_at = None;
        for (cursor, step_messages) in source.visible_steps.iter().rev() {
            retained += step_messages.len();
            start_at = Some(cursor.clone());
            if retained >= keep_messages {
                break;
            }
        }
        let mut candidate = (*current).clone();
        candidate.compact = Some(ActiveCompactMarker {
            summary: generated.summary.clone(),
            start_at,
            source_revision: source.revision.get(),
        });
        // compact 以 summary 替代窗口后，skill 注入内容不再可见，去重前提
        // （内容仍在窗口）失效：必须随窗口一起失效记录，否则同 revision 的
        // 再次加载会命中 AlreadyLoaded 拒绝重发，模型永远拿不回内容。
        candidate.skill_load_records.clear();
        candidate.revision += 1;
        candidate.updated_at = share::time::now_iso();
        self.persist_candidate(&current, &candidate)
            .await
            .map_err(ContextPortError::Compact)?;
        self.publish_generation(&current, candidate)
            .map_err(ContextPortError::SessionRepository)?;
        Ok(CompactOutcome::Committed(crate::domain::CompactResult {
            summary: generated.summary,
            recent_messages: generated.recent_messages,
            source_revision: source.revision,
            quality: generated.quality,
            metrics: Box::new(generated.metrics),
        }))
    }

    fn compact_revision_conflict(
        expected: SessionRevision,
        actual: SessionRevision,
    ) -> ContextPortError {
        ContextPortError::Compact(format!(
            "Session revision 冲突：期望 {expected:?}，实际 {actual:?}"
        ))
    }

    fn receipt(append: &ContextAppendData, revision: SessionRevision) -> SessionAppendReceiptData {
        SessionAppendReceiptData {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: revision,
        }
    }

    fn accepted_receipt(
        append: &AcceptedInputAppendData,
        revision: SessionRevision,
    ) -> SessionAppendReceiptData {
        SessionAppendReceiptData {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: revision,
        }
    }

    fn build_commit_plan(
        before: &CanonicalSession,
        after: &CanonicalSession,
    ) -> Result<SessionCommitPlan, String> {
        let manifest = crate::domain::session::SessionGenerationManifest::new(
            before.id.clone(),
            before.revision,
            before
                .run_slices
                .iter()
                .flat_map(|slice| {
                    slice
                        .steps
                        .iter()
                        .map(|step| crate::domain::session::RunStepCursor {
                            run_id: slice.run_id.clone(),
                            step_id: step.step_id.clone(),
                        })
                })
                .collect(),
        )
        .map_err(|error| error.to_string())?;
        SessionCommitPlan::between_preserving_unloaded_steps(before, after, &manifest)
            .map_err(|error| error.to_string())
    }

    async fn persist_candidate(
        &self,
        before: &CanonicalSession,
        after: &CanonicalSession,
    ) -> Result<(), String> {
        let plan = Self::build_commit_plan(before, after)?;
        match self.writer.commit(&after.id, before.revision, plan).await {
            Ok(()) => Ok(()),
            Err(commit_error) => {
                // 数据集可能被外部清空（空集 + 内存修订号 N > 0）：磁盘是空集、
                // 内存是唯一真相源，尝试以全量快照重建一次。writer 对非空
                // 数据集 fail-closed，普通 IO 错误不会被此兜底掩盖。
                match self.writer.rebuild_empty_dataset(&after.id, after).await {
                    Ok(()) => Ok(()),
                    Err(_) => Err(commit_error),
                }
            }
        }
    }

    fn publish_generation(
        &self,
        current: &Arc<CanonicalSession>,
        candidate: CanonicalSession,
    ) -> Result<(), String> {
        let mut committed = self.session.write().map_err(|error| error.to_string())?;
        #[cfg(any(test, feature = "dev"))]
        crate::adapters::session_lifecycle::record_generation_transition(current, &candidate);
        #[cfg(not(any(test, feature = "dev")))]
        let _ = current;
        *committed = Arc::new(candidate);
        Ok(())
    }
}

#[async_trait]
impl SessionRepository for CanonicalSessionRepository {
    async fn snapshot(&self, session_id: &SessionId) -> Result<SessionItem, String> {
        let session = self.session.read().map_err(|error| error.to_string())?;
        if session.id != session_id.as_str() {
            return Err(format!("Session 不存在：{session_id}"));
        }
        let structured_history = session.visible_history();
        let messages = crate::domain::ContextMessages::from_committed_steps(
            structured_history
                .iter()
                .flat_map(|slice| slice.steps.iter())
                .flat_map(|step| {
                    step.accepted_input
                        .iter()
                        .map(|input| input.messages.as_arc())
                        .chain(step.outcome.iter().map(|outcome| outcome.messages.as_arc()))
                })
                .collect(),
            Vec::new(),
        );
        Ok(SessionItem {
            revision: SessionRevision::new(session.revision),
            messages,
            structured_history: Some(structured_history),
            active_summary: session.active_summary().map(str::to_string),
        })
    }

    async fn current_revision(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<SessionRevision>, String> {
        let session = self.session.read().map_err(|error| error.to_string())?;
        if session.id != session_id.as_str() {
            return Ok(None);
        }
        Ok(Some(SessionRevision::new(session.revision)))
    }

    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppendData,
    ) -> Result<SessionAppendReceiptData, AcceptedInputError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| AcceptedInputError::Storage(error.to_string()))?
            .clone();
        if current.id != append.session_id.as_str() {
            return Err(AcceptedInputError::SessionNotFound(
                append.session_id.clone(),
            ));
        }
        let fingerprint = append.fingerprint()?;
        if let Some(input) = current.accepted_input(append.run_id.as_ref(), append.step_id.as_str())
        {
            if input.fingerprint == fingerprint.as_str() {
                return Ok(Self::accepted_receipt(
                    append,
                    SessionRevision::new(input.committed_revision),
                ));
            }
            return Err(AcceptedInputError::ContentConflict {
                run_id: append.run_id.clone(),
                step_id: append.step_id.clone(),
            });
        }
        let mut candidate = (*current).clone();
        candidate.revision += 1;
        candidate.updated_at = share::time::now_iso();
        candidate.tasks = SessionSnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SessionSnapshotState::Captured(self.workspace_persist.snapshot());
        candidate.append_accepted_input(
            append.run_id.as_ref(),
            append.step_id.as_str(),
            AcceptedInputRecord::new(
                append.messages.clone(),
                fingerprint.as_str(),
                candidate.revision,
            ),
        );
        // C 阶段：sidecar 退役——accepted input 随 Session member 一次原子提交。
        self.persist_candidate(&current, &candidate)
            .await
            .map_err(AcceptedInputError::Storage)?;
        let revision = SessionRevision::new(candidate.revision);
        self.publish_generation(&current, candidate)
            .map_err(AcceptedInputError::Storage)?;
        Ok(Self::accepted_receipt(append, revision))
    }

    async fn advance_tool_receipt(
        &self,
        mutation: ToolReceiptMutationData,
    ) -> Result<ToolReceiptMutationReceiptData, ToolReceiptMutationError> {
        let _mutation_guard = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ToolReceiptMutationError::Storage(error.to_string()))?
            .clone();
        if current.id != mutation.identity.session_id.as_str() {
            return Err(ToolReceiptMutationError::SessionNotFound(
                mutation.identity.session_id.clone(),
            ));
        }
        if let Some(receipt) = current.tool_receipt(&mutation) {
            let advanced = receipt.clone().advance(mutation.clone())?;
            if !advanced.changed {
                return Ok(advanced);
            }
        }
        let mut candidate = (*current).clone();
        let changed = candidate.advance_tool_receipt(mutation.clone())?;
        if !changed {
            let receipt = candidate
                .tool_receipt(&mutation)
                .expect("unchanged receipt must exist")
                .clone();
            return Ok(ToolReceiptMutationReceiptData {
                receipt,
                changed: false,
            });
        }
        candidate.revision += 1;
        candidate.updated_at = share::time::now_iso();
        candidate.tasks = SessionSnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SessionSnapshotState::Captured(self.workspace_persist.snapshot());
        let receipt = candidate
            .tool_receipt(&mutation)
            .expect("advanced receipt must exist")
            .clone();
        // C 阶段：sidecar 退役——receipt 变更随 Session member 一次原子提交，
        // durable 语义与普通 Session 写入一致（提交失败不发布内存态并报错）。
        self.persist_candidate(&current, &candidate)
            .await
            .map_err(ToolReceiptMutationError::Storage)?;
        self.publish_generation(&current, candidate)
            .map_err(ToolReceiptMutationError::Storage)?;
        return Ok(ToolReceiptMutationReceiptData {
            receipt,
            changed: true,
        });
    }

    async fn step_receipts(
        &self,
        session_id: &SessionId,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
    ) -> Result<Vec<crate::domain::RunStepReceiptData>, ToolReceiptMutationError> {
        let current = self
            .session
            .read()
            .map_err(|error| ToolReceiptMutationError::Storage(error.to_string()))?
            .clone();
        if current.id != session_id.as_str() {
            return Err(ToolReceiptMutationError::SessionNotFound(
                session_id.clone(),
            ));
        }
        Ok(current.step_receipts(run_id.as_ref(), step_id.as_str()))
    }

    async fn compare_and_record_skill_load(
        &self,
        mutation: tools::published::skill::SkillLoadMutation,
    ) -> Result<
        tools::published::skill::SkillLoadDecision,
        tools::published::skill::SkillLoadStateError,
    > {
        let _mutation_guard = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| {
                tools::published::skill::SkillLoadStateError::Storage(error.to_string())
            })?
            .clone();
        if current.id != mutation.session_id() {
            return Err(
                tools::published::skill::SkillLoadStateError::SessionNotFound(
                    mutation.session_id().to_string(),
                ),
            );
        }
        let mut candidate = (*current).clone();
        let decision = candidate.compare_and_record_skill(
            mutation.scope(),
            mutation.skill_name(),
            mutation.revision(),
        );
        if decision == tools::published::skill::SkillLoadDecision::AlreadyLoaded {
            return Ok(decision);
        }
        candidate.revision += 1;
        candidate.updated_at = share::time::now_iso();
        candidate.tasks = SessionSnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SessionSnapshotState::Captured(self.workspace_persist.snapshot());
        self.persist_candidate(&current, &candidate)
            .await
            .map_err(tools::published::skill::SkillLoadStateError::Storage)?;
        self.publish_generation(&current, candidate)
            .map_err(tools::published::skill::SkillLoadStateError::Storage)?;
        Ok(decision)
    }

    async fn append_finalized(
        &self,
        append: &ContextAppendData,
    ) -> Result<SessionAppendReceiptData, ContextAppendError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextAppendError::Storage(error.to_string()))?
            .clone();
        if current.id != append.session_id.as_str() {
            return Err(ContextAppendError::SessionNotFound(
                append.session_id.clone(),
            ));
        }
        let fingerprint = append.fingerprint()?;
        if let Some(committed) = current
            .committed_steps
            .find(append.run_id.as_ref(), append.step_id.as_str())
        {
            if committed.fingerprint == fingerprint.as_str() {
                let revision = SessionRevision::new(committed.committed_revision);
                return Ok(Self::receipt(append, revision));
            }
            return Err(ContextAppendError::ContentConflict {
                run_id: append.run_id.clone(),
                step_id: append.step_id.clone(),
            });
        }
        let actual = SessionRevision::new(current.revision);
        if actual != append.expected_revision {
            let receipt_only_advances = current
                .run_slices
                .iter()
                .find(|slice| slice.run_id == append.run_id.as_ref())
                .and_then(|slice| {
                    slice
                        .steps
                        .iter()
                        .find(|step| step.step_id == append.step_id.as_str())
                })
                .is_some_and(|step| step.outcome.is_none());
            if !receipt_only_advances {
                return Err(ContextAppendError::RevisionConflict {
                    expected: append.expected_revision,
                    actual,
                });
            }
        }

        let mut candidate = (*current).clone();
        candidate.revision += 1;
        candidate.updated_at = share::time::now_iso();
        candidate.tasks = SessionSnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SessionSnapshotState::Captured(self.workspace_persist.snapshot());
        candidate.append_finalized_outcome(
            append.run_id.as_ref(),
            append.step_id.as_str(),
            FinalizedOutcomeRecord {
                finalize_cause: append.finalize_cause,
                duration_ms: append.duration_ms,
                messages: append.messages.clone().into(),
                receipts: append.receipts.clone(),
                api_input_tokens: append.api_input_tokens,
                fingerprint: fingerprint.as_str().to_string(),
                committed_revision: candidate.revision,
            },
        );
        candidate.committed_steps = candidate.committed_steps.append(CommittedStep {
            run_id: append.run_id.to_string(),
            step_id: append.step_id.as_str().to_string(),
            fingerprint: fingerprint.as_str().to_string(),
            committed_revision: candidate.revision,
        });

        self.persist_candidate(&current, &candidate)
            .await
            .map_err(ContextAppendError::Storage)?;
        let revision = SessionRevision::new(candidate.revision);
        self.publish_generation(&current, candidate)
            .map_err(ContextAppendError::Storage)?;
        Ok(Self::receipt(append, revision))
    }

    async fn commit_compaction(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        // 手动压缩：跳过熔断（用户显式要求）与 revision 对齐（直接取当前）。
        if matches!(request.trigger, CompactTrigger::Manual) {
            let source = self
                .freeze_compact_source(&request.source.session_id, None)
                .await?;
            if source.messages.len() <= 4 {
                return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
            }
            let generated = match self
                .generate_compact(
                    &source,
                    request.source.context_size,
                    request.progress.clone(),
                    request.task_snapshot.as_ref(),
                    request.llm.as_ref(),
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await
            {
                Ok(Some(generated)) => generated,
                Ok(None) => {
                    return Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection));
                }
                Err(reason) => return Ok(CompactOutcome::Skipped(reason)),
            };
            return self
                .commit_generated_compact(&request.source.session_id, &source, generated)
                .await;
        }
        let Some(attempt) = self.begin_auto_compact_attempt(
            request.source.config_snapshot.auto_compact_failure_limit(),
        ) else {
            return Ok(CompactOutcome::Skipped(
                CompactSkipReason::CircuitBreakerOpen,
            ));
        };
        let result = self.commit_automatic_compaction(request).await;
        attempt.finish(&result);
        result
    }

    async fn clear(&self, session_id: &SessionId) -> Result<(), ContextPortError> {
        let _mutation = self.mutation_gate.lock().await;
        let current = self
            .session
            .read()
            .map_err(|error| ContextPortError::SessionRepository(error.to_string()))?
            .clone();
        if current.id != session_id.as_str() {
            return Err(ContextPortError::SessionNotFound(session_id.clone()));
        }
        let mut candidate = (*current).clone();
        candidate.chats.clear();
        candidate.compact = None;
        candidate.run_slices = candidate.run_slices.cleared();
        candidate.committed_steps = candidate.committed_steps.cleared();
        candidate.skill_load_records.clear();
        candidate.revision += 1;
        candidate.updated_at = share::time::now_iso();
        candidate.tasks = SessionSnapshotState::Captured(self.task_persist.collect_snapshot());
        candidate.workspace = SessionSnapshotState::Captured(self.workspace_persist.snapshot());
        // `/clear` 是逻辑断点：磁盘 step 成员无限保留，state 记录 clear
        // 边界，由 writer 按 persisted manifest 计算并返回对齐后的快照。
        let fallback = candidate.clone();
        let cleared = match self
            .writer
            .commit_clearing_history(&current, candidate)
            .await
        {
            Ok(cleared) => cleared,
            Err(commit_error) => {
                // 数据集可能被外部清空（空集 + 内存修订号 N > 0）：磁盘是空集、
                // 内存是唯一真相源，尝试以全量快照重建一次。writer 对非空
                // 数据集 fail-closed，普通 IO 错误不会被此兜底掩盖。空数据集
                // 没有可截断的持久化 step，边界保持 None。
                let mut fallback = fallback;
                fallback.cleared_after = None;
                self.writer
                    .rebuild_empty_dataset(&current.id, &fallback)
                    .await
                    .map_err(|_| ContextPortError::SessionRepository(commit_error))?;
                fallback
            }
        };
        self.publish_generation(&current, cleared)
            .map_err(ContextPortError::SessionRepository)?;
        Ok(())
    }
}
