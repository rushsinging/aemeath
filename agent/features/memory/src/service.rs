use crate::adapters::MemoryPolicy;
use crate::domain::constants::MIN_SYNTHESIS_EVIDENCE;
use crate::domain::event::{
    ConfigFingerprint, EventActor, EventChange, EventContext, EventOutcome, MemoryEvent,
    MemoryEventOp,
};
use crate::domain::*;
use crate::noop::NoopEventAppend;
use crate::ports::*;
use async_trait::async_trait;
use std::{
    sync::{Arc, Mutex as StdMutex, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

/// Injectable source of Unix time used when Reflection creates memories.
pub type MemoryClock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// 写组事件的 before/after 抓取槽：mutation 闭包可能因 CAS 重试重跑，
/// 每次整体覆盖，读出的即最终提交那一版受影响条目全文。
type WriteCapture = Arc<StdMutex<(Vec<MemoryEntry>, Vec<MemoryEntry>)>>;

/// 位置迁移类 op（archive/restore/compact）的 affected 抓取槽：
/// 正文不变只迁位置，before/after 同文，故只记一份全文列表。
type AffectedCapture = Arc<StdMutex<Vec<MemoryEntry>>>;

pub(crate) fn system_time_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// One layer's committed dataset together with the Storage revision it was read
/// or committed at. Each layer advances its own revision independently.
#[derive(Clone)]
struct LayerState<R> {
    dataset: MemoryDataset,
    revision: R,
}

#[derive(Clone)]
struct CommittedState<R> {
    global: LayerState<R>,
    project: LayerState<R>,
}

/// Durable Memory application service. Queries only inspect committed memory;
/// every mutation is serialized through one async gate and publishes only
/// after Storage reports a committed receipt. Each layer owns an independent
/// revision, so a mutation only commits the layer it actually changed.
pub(crate) struct MemoryService<S: MemoryDatasetStore> {
    store: S,
    policy: MemoryPolicy,
    state: RwLock<CommittedState<S::Revision>>,
    mutation_gate: Mutex<()>,
    clock: MemoryClock,
    /// System One 评分端口（None = 重排关闭，词法序原样返回）。
    scorer: Option<Arc<dyn systemone::ScoringPort>>,
    /// 事件追加端口（中心化 emit 的唯一出口，fail-open，见 `emit_event`）。
    events: Arc<dyn MemoryEventAppendPort>,
}

impl<S: MemoryDatasetStore> MemoryService<S> {
    pub async fn open(store: S, policy: MemoryPolicy) -> Result<Self, MemoryError> {
        Self::open_with_clock(store, policy, system_time_seconds).await
    }

    /// 带评分端口的装配入口（生产消费者：DatasetMemoryOpener 经场景开关注入）。
    pub async fn open_with_scorer(
        store: S,
        policy: MemoryPolicy,
        scorer: Option<Arc<dyn systemone::ScoringPort>>,
    ) -> Result<Self, MemoryError> {
        Self::open_with_clock_and_scorer(store, policy, system_time_seconds, scorer).await
    }

    pub async fn open_with_clock(
        store: S,
        policy: MemoryPolicy,
        clock: impl Fn() -> u64 + Send + Sync + 'static,
    ) -> Result<Self, MemoryError> {
        Self::open_with_clock_and_scorer(store, policy, clock, None).await
    }

    /// 带评分端口的装配入口：composition 在场景开关开启时注入重排能力
    ///（生产消费者随 composition wiring 落地，见 wire_memory_opener 扩展）。
    pub async fn open_with_clock_and_scorer(
        store: S,
        policy: MemoryPolicy,
        clock: impl Fn() -> u64 + Send + Sync + 'static,
        scorer: Option<Arc<dyn systemone::ScoringPort>>,
    ) -> Result<Self, MemoryError> {
        Self::open_with_clock_and_scorer_and_events(
            store,
            policy,
            clock,
            scorer,
            Arc::new(NoopEventAppend),
        )
        .await
    }

    /// 全参装配入口：测试与后续 opener 经此注入真实事件端口；
    /// 其余入口以 `NoopEventAppend` 作默认，保持既有调用方零改动。
    pub async fn open_with_clock_and_scorer_and_events(
        store: S,
        policy: MemoryPolicy,
        clock: impl Fn() -> u64 + Send + Sync + 'static,
        scorer: Option<Arc<dyn systemone::ScoringPort>>,
        events: Arc<dyn MemoryEventAppendPort>,
    ) -> Result<Self, MemoryError> {
        log::debug!(target: crate::LOG_TARGET, "open_with_clock enter");
        let outcome = Self::load_open(store, policy, clock, scorer, events).await;
        match &outcome {
            Ok(_) => log::debug!(target: crate::LOG_TARGET, "open_with_clock ok"),
            Err(error) => log::debug!(target: crate::LOG_TARGET, "open_with_clock error: {error}"),
        }
        outcome
    }

    /// Loads both layers and assembles the service without logging. The public
    /// `open_with_clock` wraps this so every open records an enter marker and a
    /// success/failure exit marker. Keeping the typed `MemoryError` unchanged
    /// means the exit log only carries the error's Display form (which never
    /// embeds memory content), never the raw error value.
    async fn load_open(
        store: S,
        policy: MemoryPolicy,
        clock: impl Fn() -> u64 + Send + Sync + 'static,
        scorer: Option<Arc<dyn systemone::ScoringPort>>,
        events: Arc<dyn MemoryEventAppendPort>,
    ) -> Result<Self, MemoryError> {
        validate_policy(policy)?;
        let global = load_layer(&store, MemoryLayer::Global).await?;
        let project = load_layer(&store, MemoryLayer::Project).await?;
        let service = Self {
            store,
            policy,
            state: RwLock::new(CommittedState { global, project }),
            mutation_gate: Mutex::new(()),
            clock: Arc::new(clock),
            scorer,
            events,
        };
        service.emit_open_load().await;
        service.emit_assembly_fingerprint().await;
        Ok(service)
    }

    /// OpenLoad：两层加载成功后发射一条覆盖双层的生命周期事件（actor=Opener，
    /// layer=None）。context 用两层 active/archive 计数摘要——打开不改语料，
    /// dump 全量条目既无必要也不便宜，计数足以复盘「打开时装了什么」。
    async fn emit_open_load(&self) {
        let (global_active, global_archive, project_active, project_archive) = {
            let state = self.state.read().expect("committed state lock poisoned");
            (
                state.global.dataset.active().len(),
                state.global.dataset.archive().len(),
                state.project.dataset.active().len(),
                state.project.dataset.archive().len(),
            )
        };
        let mut event = self.build_lifecycle_event(
            MemoryEventOp::OpenLoad,
            "open_load",
            format!("open_load-{}", uuid::Uuid::now_v7()),
            EventActor::Opener,
        );
        event.context.trigger_summary = Some(format!(
            "global active={global_active} archive={global_archive}; \
             project active={project_active} archive={project_archive}"
        ));
        self.emit_event(event).await;
    }

    /// AssemblyFingerprint：打开完成后记录评分/阈值/策略指纹，供复盘对照「当时装配」。
    async fn emit_assembly_fingerprint(&self) {
        let mut event = self.build_lifecycle_event(
            MemoryEventOp::AssemblyFingerprint,
            "assembly_fingerprint",
            format!("assembly_fingerprint-{}", uuid::Uuid::now_v7()),
            EventActor::Opener,
        );
        event.config_fingerprint = ConfigFingerprint {
            scoring_enabled: self.scorer.is_some(),
            similarity_threshold: Some(self.policy.similarity_threshold),
            ..ConfigFingerprint::default()
        };
        event.context.trigger_summary = Some(format!(
            "max_entries={};scoring={}",
            self.policy.max_entries,
            self.scorer.is_some()
        ));
        self.emit_event(event).await;
    }

    /// 生命周期事件组装（open_load / commit_cas …）：`correlation_id` 由调用方
    /// 给定并复用为 `event_id`；`affected` 恒为空——受影响条目全文由写组事件
    /// 承载，生命周期只记坐标与 outcome。
    fn build_lifecycle_event(
        &self,
        op: MemoryEventOp,
        stage: &str,
        correlation_id: String,
        actor: EventActor,
    ) -> MemoryEvent {
        MemoryEvent::new(
            correlation_id.clone(),
            (self.clock)() * 1000,
            op,
            EventOutcome::Succeeded,
            correlation_id,
            actor,
            EventChange::Lifecycle {
                stage: stage.to_string(),
                affected: vec![],
            },
            EventContext::default(),
            ConfigFingerprint {
                scoring_enabled: self.scorer.is_some(),
                ..ConfigFingerprint::default()
            },
        )
    }

    /// 中心化事件发射：追加失败仅记录一条无正文的告警并吞掉错误——
    /// **NEVER** 把 append 错误传播给调用方（fail-open，事件流绝不阻断主路径）。
    async fn emit_event(&self, event: MemoryEvent) {
        if let Err(error) = self.events.append(&event).await {
            log::warn!(
                target: crate::LOG_TARGET,
                "memory_event_append_failed op={:?} err={error}",
                event.op
            );
        }
    }

    /// 写组通用事件组装：`event_id` 与 `correlation_id` 由调用方给出
    /// （一次逻辑调用一条 correlation；CAS 重试收敛进同一条 emit 天然共享）。
    /// `change` 必须携带受影响条目全文（NEVER 仅 id）；layer 坐标由调用方补。
    fn build_write_event(
        &self,
        op: MemoryEventOp,
        event_id: String,
        correlation_id: String,
        change: EventChange,
    ) -> MemoryEvent {
        MemoryEvent::new(
            event_id,
            (self.clock)() * 1000,
            op,
            EventOutcome::Succeeded,
            correlation_id,
            EventActor::Service,
            change,
            EventContext::default(),
            ConfigFingerprint {
                scoring_enabled: self.scorer.is_some(),
                ..ConfigFingerprint::default()
            },
        )
    }

    /// 读组通用事件组装：候选集携带 `MemoryEntry` 全文（Q3=A）；
    /// `context` 由调用方填 query / stats 摘要等。
    fn build_read_event(
        &self,
        op: MemoryEventOp,
        event_id: String,
        correlation_id: String,
        change: EventChange,
        context: EventContext,
    ) -> MemoryEvent {
        MemoryEvent::new(
            event_id,
            (self.clock)() * 1000,
            op,
            EventOutcome::Succeeded,
            correlation_id,
            EventActor::Service,
            change,
            context,
            ConfigFingerprint {
                scoring_enabled: self.scorer.is_some(),
                ..ConfigFingerprint::default()
            },
        )
    }

    /// 组装一条 write-add 事件快照（变更快照携带新增条目全文）。
    fn build_write_add_event(&self, entry: &MemoryEntry, id: MemoryId) -> MemoryEvent {
        let mut event = self.build_write_event(
            MemoryEventOp::WriteAdd,
            format!("write_add-{id}"),
            format!("write_add-{id}"),
            EventChange::Write {
                before: vec![],
                after: vec![entry.clone()],
            },
        );
        event.layer = Some(entry.layer);
        event
    }

    /// update / pin / mark_outdated 共用的单条目变更落点：mutation 在闭包内
    /// 抓取变更前后全文，提交成功后 emit `op`（key 进 event_id / correlation）。
    async fn mutate_entry_and_emit(
        &self,
        op: MemoryEventOp,
        key: &'static str,
        id: MemoryId,
        mutation: impl Fn(&mut MemoryEntry) + Send + Sync + 'static,
    ) -> Result<bool, MemoryError> {
        let slot: WriteCapture = Default::default();
        let capture = Arc::clone(&slot);
        let changed = self
            .mutate_owning_layer(move |dataset| {
                mutate_active(dataset, &id, |entry| {
                    let before = entry.clone();
                    mutation(entry);
                    let after = entry.clone();
                    *capture.lock().expect("write event slot poisoned") =
                        (vec![before], vec![after]);
                })
            })
            .await?;
        if changed {
            let (before, after) = slot.lock().expect("write event slot poisoned").clone();
            let layer = after
                .first()
                .or_else(|| before.first())
                .map(|entry| entry.layer);
            let correlation = format!("{key}-{id}-{}", uuid::Uuid::now_v7());
            let mut event = self.build_write_event(
                op,
                correlation.clone(),
                correlation,
                EventChange::Write { before, after },
            );
            event.layer = layer;
            self.emit_event(event).await;
        }
        Ok(changed)
    }

    /// 评分开启时重排词法召回的 top-N；评分失败静默回退词法序（NEVER 阻断搜索）。
    async fn rerank_if_scored(
        &self,
        query: &MemorySearchQuery,
        hits: Vec<MemorySearchHit>,
    ) -> Vec<MemorySearchHit> {
        let Some(scorer) = &self.scorer else {
            return hits;
        };
        let top_n = hits.len().min(crate::constants::RERANK_TOP_N);
        let top_hits = &hits[..top_n]; // allow unsafe_text_op: Vec slice (MemorySearchHit)
        let Some((state, question)) =
            crate::domain::rerank::build_rerank_request(&query.text, top_hits)
        else {
            return hits;
        };
        let reranked = match scorer.answer(&state, &[question]).await {
            Ok(answers) => match answers.first() {
                Some(systemone::ScoringAnswer::Choice { probabilities, .. }) => {
                    crate::domain::rerank::apply_rerank_order(hits, top_n, probabilities)
                }
                _ => hits,
            },
            Err(unavailable) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "memory_rerank_fallback reason={unavailable}"
                );
                hits
            }
        };
        let mut reranked = reranked;
        reranked.truncate(query.limit);
        reranked
    }

    /// Serializes and commits a change scoped to exactly one layer. Only the
    /// changed layer is committed, and a single stale-CAS conflict refreshes and
    /// recomputes this layer once before publishing a committed receipt.
    async fn mutate_layer<T, F>(&self, layer: MemoryLayer, operation: F) -> Result<T, MemoryError>
    where
        F: Fn(&mut MemoryDataset) -> Result<(T, bool), MemoryError>,
    {
        let _permit = self.mutation_gate.lock().await;
        // 同一次 mutate_layer 调用（含 CAS 冲突重试）共用一条 CommitCas 因果链。
        let commit_correlation = format!("commit_cas-{layer:?}-{}", uuid::Uuid::now_v7());
        for attempt in 0..=1 {
            let mut candidate = self.layer_state(layer);
            let (output, changed) = operation(&mut candidate.dataset)?;
            if !changed {
                return Ok(output);
            }
            match self
                .store
                .commit(layer, &candidate.revision, &candidate.dataset)
                .await
            {
                Ok(receipt) => {
                    let commit_revision = S::event_revision_label(receipt.revision());
                    // Visible and RecoveryPending are both committed receipts.
                    candidate.revision = receipt.into_revision();
                    self.set_layer_state(layer, candidate);
                    let mut event = self.build_lifecycle_event(
                        MemoryEventOp::CommitCas,
                        "commit_cas",
                        commit_correlation,
                        EventActor::Service,
                    );
                    event.layer = Some(layer);
                    event.commit_revision = commit_revision;
                    self.emit_event(event).await;
                    return Ok(output);
                }
                Err(error) if is_concurrent_write(&error) && attempt == 0 => {
                    let refreshed = load_layer(&self.store, layer).await?;
                    self.set_layer_state(layer, refreshed);
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("the mutation retry loop has exactly two attempts")
    }

    /// Applies an entry-targeted mutation to whichever layer currently holds the
    /// entry. The entry lives in exactly one layer, so at most one layer is
    /// committed; a non-matching layer is a no-op and never commits.
    async fn mutate_owning_layer<F>(&self, operation: F) -> Result<bool, MemoryError>
    where
        F: Fn(&mut MemoryDataset) -> bool,
    {
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let changed = self
                .mutate_layer(layer, |dataset| {
                    let changed = operation(dataset);
                    Ok((changed, changed))
                })
                .await?;
            if changed {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 承载取代关系：`superseded` 中每条被 `superseding` 取代的记忆各建立
    /// 一条 `superseded_by` 边，返回成功建立的数量。
    ///
    /// 每条关系是一次独立的跨层 mutation，环检测在写入前基于**提交态**
    /// 快照做判定（M9）；被拒的关系既不写入也不计数，由调用方通过
    /// `attempted - completed` 表达跳过。
    async fn establish_supersede_relations(
        &self,
        superseding: MemoryId,
        superseded: &[MemoryId],
    ) -> usize {
        let mut established = 0;
        for target in superseded {
            if self.mark_superseded_by(*target, superseding).await {
                established += 1;
            }
        }
        established
    }

    async fn mark_superseded_by(&self, target: MemoryId, superseding: MemoryId) -> bool {
        let (global, project) = self.snapshot();
        let chain = [
            global.active(),
            global.archive(),
            project.active(),
            project.archive(),
        ];
        if would_create_supersede_cycle(&superseding, &target, supersede_chain_of(&chain)) {
            log::info!(
                target: crate::LOG_TARGET,
                "memory_supersede_rejected target={} superseding={} reason=cycle",
                target,
                superseding,
            );
            return false;
        }
        let slot: WriteCapture = Default::default();
        let capture = Arc::clone(&slot);
        match self
            .mutate_owning_layer(move |dataset| {
                // assign 会改写 superseded_by：先取 before 全文，再落关系。
                let before = lookup_entry(dataset, &target);
                let changed = assign_supersede(dataset.active_mut(), &target, superseding)
                    || assign_supersede(dataset.archive_mut(), &target, superseding);
                if changed {
                    let after = lookup_entry(dataset, &target);
                    if let (Some(before), Some(after)) = (before, after) {
                        *capture.lock().expect("supersede event slot poisoned") =
                            (vec![before], vec![after]);
                    }
                }
                changed
            })
            .await
        {
            Ok(true) => {
                // 取代关系建立成功：before=未被取代全文，after=带
                // superseded_by 的全文（SupersedeSynthesis 的关系侧落点）。
                let (before, after) = slot.lock().expect("supersede event slot poisoned").clone();
                let layer = after
                    .first()
                    .or_else(|| before.first())
                    .map(|entry| entry.layer);
                let correlation =
                    format!("supersede-{target}-{superseding}-{}", uuid::Uuid::now_v7());
                let mut event = self.build_write_event(
                    MemoryEventOp::SupersedeSynthesis,
                    correlation.clone(),
                    correlation,
                    EventChange::Write { before, after },
                );
                event.layer = layer;
                self.emit_event(event).await;
                true
            }
            Ok(false) => {
                log::info!(
                    target: crate::LOG_TARGET,
                    "memory_supersede_rejected target={} superseding={} reason=missing_target",
                    target,
                    superseding,
                );
                false
            }
            Err(error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "memory_supersede_failed target={} superseding={} error={error}",
                    target,
                    superseding,
                );
                false
            }
        }
    }

    fn layer_state(&self, layer: MemoryLayer) -> LayerState<S::Revision> {
        let state = self.state.read().expect("memory state lock poisoned");
        match layer {
            MemoryLayer::Global => state.global.clone(),
            MemoryLayer::Project => state.project.clone(),
        }
    }

    fn set_layer_state(&self, layer: MemoryLayer, new: LayerState<S::Revision>) {
        let mut state = self.state.write().expect("memory state lock poisoned");
        match layer {
            MemoryLayer::Global => state.global = new,
            MemoryLayer::Project => state.project = new,
        }
    }

    fn snapshot(&self) -> (MemoryDataset, MemoryDataset) {
        let state = self.state.read().expect("memory state lock poisoned");
        (state.global.dataset.clone(), state.project.dataset.clone())
    }

    /// 读路径统一入口：现读磁盘最新已提交数据（#1886）。
    ///
    /// 每层独立「初读 + 撞错重读一次」；任一层最终失败则整体回退内存快照，
    /// 保证读方法永不因存储抖动向上抛错。结果不写回 `state`——写路径的
    /// CAS 基准仍由 `mutate_layer` 自持，避免读路径与写路径竞争导致 revision 回退。
    async fn load_latest_layers(&self) -> (MemoryDataset, MemoryDataset) {
        match self.try_load_latest_layers().await {
            Some(layers) => layers,
            None => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "memory_read_fallback reason=store_unavailable action=snapshot_fallback"
                );
                self.snapshot()
            }
        }
    }

    async fn try_load_latest_layers(&self) -> Option<(MemoryDataset, MemoryDataset)> {
        let global = self.load_layer_with_retry(MemoryLayer::Global).await?;
        let project = self.load_layer_with_retry(MemoryLayer::Project).await?;
        Some((global.dataset, project.dataset))
    }

    async fn load_layer_with_retry(&self, layer: MemoryLayer) -> Option<LayerState<S::Revision>> {
        let first_error = match load_layer(&self.store, layer).await {
            Ok(dataset) => return Some(dataset),
            Err(error) => error,
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "memory_read_retry layer={:?} first_error={:?}",
            layer,
            first_error
        );
        match load_layer(&self.store, layer).await {
            Ok(dataset) => Some(dataset),
            Err(second_error) => {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "memory_read_retry_failed layer={:?} second_error={:?}",
                    layer,
                    second_error
                );
                None
            }
        }
    }

    /// Compacts a single layer as one observable mutation, archiving entries
    /// that exceed the policy budget and reporting that layer's totals.
    /// 归档超出容量上限的条目；`correlation` 由 `compact` 下发，让两层
    /// commit 的事件共享同一条因果链。
    async fn compact_layer(
        &self,
        layer: MemoryLayer,
        correlation: &str,
    ) -> Result<CompactResult, MemoryError> {
        let policy = self.policy;
        let slot: AffectedCapture = Default::default();
        let capture = Arc::clone(&slot);
        let result = self
            .mutate_layer(layer, move |dataset| {
                let excess = dataset.active().len().saturating_sub(policy.max_entries);
                let now = dataset
                    .active()
                    .iter()
                    .map(|entry| entry.last_confirmed_at)
                    .max()
                    .unwrap_or(0);
                let ids = eviction_candidates(dataset.active(), excess, now)
                    .into_iter()
                    .map(|candidate| candidate.entry.id)
                    .collect::<Vec<_>>();
                let mut moved = Vec::new();
                dataset.active_mut().retain(|entry| {
                    if ids.contains(&entry.id) {
                        moved.push(entry.clone());
                        false
                    } else {
                        true
                    }
                });
                let archived = moved.len();
                if archived > 0 {
                    // 事件快照：本次迁出 active 的条目全文（CAS 重试整体覆盖）。
                    *capture.lock().expect("compact event slot poisoned") = moved.clone();
                }
                dataset.archive_mut().extend(moved);
                Ok((
                    CompactResult {
                        archived,
                        remaining: dataset.active().len(),
                    },
                    archived > 0,
                ))
            })
            .await?;
        if result.archived > 0 {
            // stage="compact"；affected=本次迁入 archive 的条目全文。
            let affected = slot.lock().expect("compact event slot poisoned").clone();
            let mut event = self.build_write_event(
                MemoryEventOp::Compact,
                format!("{correlation}-{layer:?}"),
                correlation.to_string(),
                EventChange::Lifecycle {
                    stage: "compact".to_string(),
                    affected,
                },
            );
            event.layer = Some(layer);
            self.emit_event(event).await;
        }
        Ok(result)
    }
}

#[async_trait]
impl<S: MemoryDatasetStore> MemoryPort for MemoryService<S> {
    async fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult {
        let (global, project) = self.load_latest_layers().await;
        let eligible_global = global
            .active()
            .iter()
            .filter(|entry| matches_filters(entry, query.layer, query.category))
            .filter(|entry| is_injection_eligible(entry, query.now))
            .count();
        let eligible_project = project
            .active()
            .iter()
            .filter(|entry| matches_filters(entry, query.layer, query.category))
            .filter(|entry| is_injection_eligible(entry, query.now))
            .count();
        let mut entries = global
            .active()
            .iter()
            .chain(project.active())
            .filter(|entry| matches_filters(entry, query.layer, query.category))
            .filter(|entry| is_injection_eligible(entry, query.now))
            .cloned()
            .collect::<Vec<_>>();
        order_for_injection(&mut entries, query.now);
        entries.truncate(query.limit);
        log::debug!(
            target: crate::LOG_TARGET,
            "memory_injection candidates={} hits={} global_candidates={} project_candidates={} limit={}",
            eligible_global.saturating_add(eligible_project),
            entries.len(),
            eligible_global,
            eligible_project,
            query.limit
        );
        let hits: Vec<MemorySearchHit> = entries
            .into_iter()
            .map(|entry| MemorySearchHit {
                entry,
                location: MemoryLocation::Active,
                outdated: false,
                ttl_expired: false,
                superseded_by: None,
                relevance: None,
            })
            .collect();
        let candidates: Vec<MemoryEntry> = hits.iter().map(|hit| hit.entry.clone()).collect();
        let correlation = format!("retrieve_for_inject-{}", (self.clock)());
        self.emit_event(self.build_read_event(
            MemoryEventOp::RetrieveForInject,
            correlation.clone(),
            correlation,
            EventChange::Read {
                candidates,
                hit_count: hits.len() as u32,
                limit: query.limit as u32,
                layer_filter: query.layer,
            },
            EventContext::default(),
        ))
        .await;
        MemorySearchResult {
            mode: MemoryRetrievalMode::InjectionPriority,
            hits,
        }
    }

    async fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult {
        let (global, project) = self.load_latest_layers().await;
        // 评分开启时扩大词法召回（重排后可被提升的候选不局限于 query.limit）。
        let recall_limit = if self.scorer.is_some() {
            query.limit.max(crate::constants::RERANK_RECALL_LIMIT)
        } else {
            query.limit
        };
        let recall_query = MemorySearchQuery {
            text: query.text.clone(),
            limit: recall_limit,
            layer: query.layer,
            category: query.category,
            include_archive: query.include_archive,
            now: query.now,
        };
        let active = global
            .active()
            .iter()
            .chain(project.active())
            .map(|entry| (entry, MemoryLocation::Active));
        let archive = global
            .archive()
            .iter()
            .chain(project.archive())
            .map(|entry| (entry, MemoryLocation::Archive));
        let candidates = if query.include_archive {
            active.chain(archive).collect::<Vec<_>>()
        } else {
            active.collect::<Vec<_>>()
        }
        .into_iter()
        .filter(|(entry, _)| matches_filters(entry, query.layer, query.category));
        let candidate_count = candidates.clone().count();
        let hits = crate::domain::lexical_search::rank_explicit_search(candidates, &recall_query);
        let hits = self.rerank_if_scored(query, hits).await;
        let min_relevance = hits
            .iter()
            .filter_map(|hit| hit.relevance)
            .reduce(f64::min)
            .unwrap_or(0.0);
        let max_relevance = hits
            .iter()
            .filter_map(|hit| hit.relevance)
            .reduce(f64::max)
            .unwrap_or(0.0);
        log::debug!(
            target: crate::LOG_TARGET,
            "memory_search query_chars={} candidates={} hits={} empty={} layer_filter={} category_filter={} include_archive={} relevance_min={:.6} relevance_max={:.6}",
            query.text.chars().count(),
            candidate_count,
            hits.len(),
            hits.is_empty(),
            query.layer.is_some(),
            query.category.is_some(),
            query.include_archive,
            min_relevance,
            max_relevance
        );
        let candidates: Vec<MemoryEntry> = hits.iter().map(|hit| hit.entry.clone()).collect();
        let correlation = format!("search-{}", (self.clock)());
        self.emit_event(self.build_read_event(
            MemoryEventOp::Search,
            correlation.clone(),
            correlation,
            EventChange::Read {
                candidates,
                hit_count: hits.len() as u32,
                limit: query.limit as u32,
                layer_filter: query.layer,
            },
            EventContext {
                query: Some(query.text.clone()),
                ..EventContext::default()
            },
        ))
        .await;
        MemorySearchResult {
            mode: MemoryRetrievalMode::ExplicitSearch,
            hits,
        }
    }

    async fn write(&self, entry: MemoryEntry) -> Result<WriteResult, MemoryError> {
        validate_content(&entry.content)?;
        let policy = self.policy;
        let layer = entry.layer;
        // Commit closure consumes `entry`; keep one snapshot for the event.
        let event_entry = entry.clone();
        // Merged 分支的 before/after 抓取槽（CAS 重试重跑闭包时整体覆盖）。
        let merged_slot: WriteCapture = Default::default();
        let merged_capture = Arc::clone(&merged_slot);
        let result = self
            .mutate_layer(layer, move |dataset| {
                if dataset
                    .active()
                    .iter()
                    .chain(dataset.archive())
                    .any(|stored| stored.id == entry.id)
                {
                    return Err(MemoryError::InvalidEntry {
                        message: "记忆 ID 必须唯一".to_string(),
                    });
                }
                // M11: every evidence pointer must resolve on this layer from
                // day one — a pointer that dangles now can only get worse.
                if entry.evidence.iter().any(|source| {
                    !dataset
                        .active()
                        .iter()
                        .chain(dataset.archive())
                        .any(|stored| &stored.id == source)
                }) {
                    return Err(MemoryError::InvalidEntry {
                        message: "证据指针指向不存在的记忆".to_string(),
                    });
                }
                let dedup_hit = dataset
                    .active()
                    .iter()
                    .find(|stored| {
                        jaccard_similarity(&stored.content, &entry.content)
                            >= policy.similarity_threshold
                    })
                    .map(|existing| existing.id);
                if let Some(existing_id) = dedup_hit {
                    // #1775: the incoming entry is archived on the same layer
                    // instead of being dropped, and the survivor keeps a
                    // pointer to it. Confirmation semantics are unchanged.
                    // The closure is `Fn` (CAS retries re-run it), so archive a
                    // clone and keep `entry` alive for the next attempt.
                    let incoming_id = entry.id;
                    // 事件快照：改写前的存活条目；改写后为存活条目 + 归档来件。
                    let before_survivor = dataset
                        .active()
                        .iter()
                        .find(|stored| stored.id == existing_id)
                        .cloned();
                    if let Some(existing) = dataset
                        .active_mut()
                        .iter_mut()
                        .find(|stored| stored.id == existing_id)
                    {
                        let mut tags = entry.tags.clone();
                        existing.tags.append(&mut tags);
                        existing.tags.sort();
                        existing.tags.dedup();
                        existing.last_confirmed_at = entry.created_at;
                        existing.confirmation_count = existing.confirmation_count.saturating_add(1);
                        existing.evidence.push(incoming_id);
                    }
                    dataset.archive_mut().push(entry.clone());
                    let after_survivor = dataset
                        .active()
                        .iter()
                        .find(|stored| stored.id == existing_id)
                        .cloned();
                    if let (Some(before), Some(after)) = (before_survivor, after_survivor) {
                        *merged_capture.lock().expect("write event slot poisoned") =
                            (vec![before], vec![after, entry.clone()]);
                    }
                    return Ok((WriteResult::Merged { existing_id }, true));
                }
                if dataset.active().len() >= policy.max_entries {
                    return Ok((
                        WriteResult::NeedsEviction {
                            candidates: eviction_candidates(dataset.active(), 3, entry.created_at),
                        },
                        false,
                    ));
                }
                let id = entry.id;
                dataset.active_mut().push(entry.clone());
                Ok((WriteResult::Added { id }, true))
            })
            .await?;
        let (outcome, eviction_candidates) = match &result {
            WriteResult::Added { .. } => ("added", 0),
            WriteResult::Merged { .. } => ("merged", 0),
            WriteResult::NeedsEviction { candidates } => ("needs_eviction", candidates.len()),
            WriteResult::NoOp => ("noop", 0),
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "memory_write outcome={} eviction_candidates={}",
            outcome,
            eviction_candidates
        );
        // One real emit on the committed add path; fail-open inside emit_event.
        if let WriteResult::Added { id } = &result {
            let event = self.build_write_add_event(&event_entry, *id);
            self.emit_event(event).await;
        }
        // 合并改写了存活条目并归档来件：WriteAdd 只留给 Added，这里以
        // Update 记录 before/after 全文（存活条目 + 归档来件）。
        if let WriteResult::Merged { existing_id } = &result {
            let (before, after) = merged_slot
                .lock()
                .expect("write event slot poisoned")
                .clone();
            if !after.is_empty() {
                let correlation = format!("write_merge-{existing_id}-{}", uuid::Uuid::now_v7());
                let mut event = self.build_write_event(
                    MemoryEventOp::Update,
                    correlation.clone(),
                    correlation,
                    EventChange::Write { before, after },
                );
                event.layer = Some(layer);
                self.emit_event(event).await;
            }
        }
        if let WriteResult::NeedsEviction { candidates } = &result {
            let affected: Vec<MemoryEntry> = candidates
                .iter()
                .map(|candidate| candidate.entry.clone())
                .collect();
            let correlation = format!("eviction_watermark-write-{}", uuid::Uuid::now_v7());
            let mut event = self.build_lifecycle_event(
                MemoryEventOp::EvictionWatermark,
                "eviction_watermark",
                correlation,
                EventActor::Service,
            );
            event.change = EventChange::Lifecycle {
                stage: "eviction_watermark".to_string(),
                affected,
            };
            event.context.trigger_summary =
                Some(format!("path=write;candidates={}", candidates.len()));
            self.emit_event(event).await;
        }
        Ok(result)
    }

    async fn update(&self, id: &MemoryId, content: &str) -> Result<bool, MemoryError> {
        validate_content(content)?;
        let content = content.to_string();
        self.mutate_entry_and_emit(MemoryEventOp::Update, "update", *id, move |entry| {
            entry.content.clone_from(&content);
        })
        .await
    }

    async fn delete(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        let id = *id;
        let slot: WriteCapture = Default::default();
        let capture = Arc::clone(&slot);
        let removed = self
            .mutate_owning_layer(move |dataset| {
                let before = dataset
                    .active()
                    .iter()
                    .filter(|entry| entry.id == id)
                    .cloned()
                    .collect::<Vec<_>>();
                let size_before = dataset.active().len();
                dataset.active_mut().retain(|entry| entry.id != id);
                let changed = size_before != dataset.active().len();
                if changed {
                    // 被删条目全文必须落在 before；after 为空（条目已消失）。
                    *capture.lock().expect("delete event slot poisoned") = (before, vec![]);
                }
                changed
            })
            .await?;
        if removed {
            let (before, after) = slot.lock().expect("delete event slot poisoned").clone();
            let layer = before.first().map(|entry| entry.layer);
            let correlation = format!("delete-{id}-{}", uuid::Uuid::now_v7());
            let mut event = self.build_write_event(
                MemoryEventOp::Delete,
                correlation.clone(),
                correlation,
                EventChange::Write { before, after },
            );
            event.layer = layer;
            self.emit_event(event).await;
        }
        Ok(removed)
    }

    async fn pin(&self, id: &MemoryId, pinned: bool) -> Result<bool, MemoryError> {
        self.mutate_entry_and_emit(MemoryEventOp::Pin, "pin", *id, move |entry| {
            entry.pinned = pinned;
        })
        .await
    }

    async fn mark_outdated(&self, id: &MemoryId) -> Result<bool, MemoryError> {
        self.mutate_entry_and_emit(MemoryEventOp::MarkOutdated, "mark_outdated", *id, |entry| {
            entry.outdated = true;
        })
        .await
    }

    async fn apply_reflection(
        &self,
        output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError> {
        // Validate the whole model-produced batch before the first durable write.
        // This prevents malformed later suggestions/ids from causing the common
        // form of partial application.
        let mut prepared = Vec::with_capacity(output.suggested_memories.len());
        for suggestion in &output.suggested_memories {
            let now = (self.clock)();
            let id = reflection_memory_id(now)?;
            let mut entry = MemoryEntry::new(
                id,
                now,
                suggestion.layer,
                suggestion.category,
                suggestion.content.clone(),
                MemorySource::Llm,
            )?;
            entry.tags = suggestion.tags.clone();
            // M13: fewer than two sources is a copy, not a synthesis. The
            // content is still written — only the type marking is withheld.
            if !apply_synthesis(&mut entry, &suggestion.synthesizes)
                && !suggestion.synthesizes.is_empty()
            {
                log::info!(
                    target: crate::LOG_TARGET,
                    "memory_synthesis_downgraded sources={} below_min={}",
                    suggestion.synthesizes.len(),
                    MIN_SYNTHESIS_EVIDENCE,
                );
            }
            prepared.push((entry, suggestion.supersedes.clone()));
        }
        // 非法引用（模型编造的标签 slug、行格式等）逐条跳过并记录，NEVER 让
        // 单条坏引用作废整批建议——事故形态即「一条非法 outdated id
        // 触发整批 apply 失败」，连合法建议一并丢弃。
        let mut outdated = Vec::with_capacity(output.outdated_memories.len());
        for token in &output.outdated_memories {
            match MemoryId::new(token) {
                Ok(id) => outdated.push(id),
                Err(_) => log::warn!(
                    target: crate::LOG_TARGET,
                    "memory_reflection_reference_skipped field=outdated token={} len={}",
                    crate::domain::truncate_reference_token(token, 40),
                    token.chars().count(),
                ),
            }
        }

        let mut result = ReflectionApplyResult {
            attempted: prepared.len() + outdated.len(),
            ..ReflectionApplyResult::default()
        };
        for (entry, supersedes) in prepared {
            let policy = self.policy;
            let layer = entry.layer;
            // 归纳产物（kind=Synthesized）落地是 SupersedeSynthesis 的写入侧
            // 落点：写前快照与产物克隆仅在归纳分支取，普通建议不付克隆成本。
            let synthesis =
                (entry.kind == MemoryKind::Synthesized).then(|| (self.snapshot(), entry.clone()));
            let write_result = match self
                .mutate_layer(layer, move |dataset| {
                    apply_reflection_entry(dataset, &entry, policy)
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Err(partial_apply_or(error, &result)),
            };
            // The relation must point at the entry that actually survived: a
            // merged suggestion never keeps its own id, so attaching to the
            // freshly generated one would dangle.
            let surviving_id = match &write_result {
                WriteResult::Added { id } => Some(*id),
                WriteResult::Merged { existing_id } => Some(*existing_id),
                WriteResult::NeedsEviction { .. } => {
                    return Err(partial_apply_or(reflection_capacity_error(), &result));
                }
                WriteResult::NoOp => None,
            };
            // 归纳产物写入已提交（Added/Merged）：emit SupersedeSynthesis，
            // change 携带全文——新增为 before 空 + after 产物；合并为
            // before 存活者旧文 + after 存活者新文与归档产物。
            if let Some((pre_snapshot, synth_entry)) = &synthesis {
                let (before, after) = match &write_result {
                    WriteResult::Added { .. } => (vec![], vec![synth_entry.clone()]),
                    WriteResult::Merged { existing_id } => {
                        let before = lookup_snapshot(pre_snapshot, existing_id)
                            .into_iter()
                            .collect();
                        let mut after: Vec<MemoryEntry> =
                            lookup_snapshot(&self.snapshot(), existing_id)
                                .into_iter()
                                .collect();
                        after.push(synth_entry.clone());
                        (before, after)
                    }
                    _ => (vec![], vec![]),
                };
                if !before.is_empty() || !after.is_empty() {
                    let correlation =
                        format!("synthesis-{}-{}", synth_entry.id, uuid::Uuid::now_v7());
                    let mut event = self.build_write_event(
                        MemoryEventOp::SupersedeSynthesis,
                        correlation.clone(),
                        correlation,
                        EventChange::Write { before, after },
                    );
                    event.layer = Some(layer);
                    self.emit_event(event).await;
                }
            }
            if surviving_id.is_some() {
                result.suggestions_added += 1;
            }
            result.completed += 1;
            if let Some(surviving_id) = surviving_id {
                result.attempted += supersedes.len();
                let established = self
                    .establish_supersede_relations(surviving_id, &supersedes)
                    .await;
                result.completed += established;
                result.superseded += established;
            }
        }
        for id in outdated {
            match self.mark_outdated(&id).await {
                Ok(marked) => {
                    if marked {
                        result.outdated_marked += 1;
                    } else {
                        // 目标在反射运行期间被删除或已过期（引用解析后库变化）：
                        // 静默跳过并记录，NEVER 失败整批。
                        log::info!(
                            target: crate::LOG_TARGET,
                            "memory_reflection_outdated_noop id={id}",
                        );
                    }
                    result.completed += 1;
                }
                Err(error) => return Err(partial_apply_or(error, &result)),
            }
        }
        Ok(result)
    }

    async fn archive(&self, ids: &[MemoryId]) -> Result<bool, MemoryError> {
        // The ids may span both layers; archive each layer as its own observable
        // mutation so a stale-CAS conflict is scoped to a single layer.
        // 一次逻辑 archive 共用一条 correlation；每层 commit 各 emit 一条。
        let correlation = format!("archive-{}", uuid::Uuid::now_v7());
        let mut archived = false;
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let ids = ids.to_vec();
            let slot: AffectedCapture = Default::default();
            let capture = Arc::clone(&slot);
            let layer_archived = self
                .mutate_layer(layer, move |dataset| {
                    let mut moved = Vec::new();
                    dataset.active_mut().retain(|entry| {
                        if ids.contains(&entry.id) && !entry.pinned {
                            moved.push(entry.clone());
                            false
                        } else {
                            true
                        }
                    });
                    let changed = !moved.is_empty();
                    if changed {
                        *capture.lock().expect("archive event slot poisoned") = moved.clone();
                    }
                    dataset.archive_mut().extend(moved);
                    Ok((changed, changed))
                })
                .await?;
            if layer_archived {
                // 同一 op，stage="archive" 与 restore 区分；正文不变仅迁位置，
                // affected 记迁移条目全文。
                let affected = slot.lock().expect("archive event slot poisoned").clone();
                let mut event = self.build_write_event(
                    MemoryEventOp::ArchiveRestore,
                    format!("{correlation}-{layer:?}"),
                    correlation.clone(),
                    EventChange::Lifecycle {
                        stage: "archive".to_string(),
                        affected,
                    },
                );
                event.layer = Some(layer);
                self.emit_event(event).await;
            }
            archived |= layer_archived;
        }
        Ok(archived)
    }

    async fn restore(&self, id: &MemoryId) -> Result<RestoreResult, MemoryError> {
        let id = *id;
        // 一次逻辑 restore 一条 correlation（命中的层唯一）。
        let correlation = format!("restore-{id}-{}", uuid::Uuid::now_v7());
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let policy = self.policy;
            let slot: AffectedCapture = Default::default();
            let capture = Arc::clone(&slot);
            let outcome = self
                .mutate_layer(layer, move |dataset| {
                    let Some(archived) = dataset
                        .archive()
                        .iter()
                        .find(|entry| entry.id == id)
                        .cloned()
                    else {
                        return Ok((RestoreResult::NotFound, false));
                    };
                    if dataset.active().len() >= policy.max_entries {
                        return Ok((
                            RestoreResult::NeedsEviction {
                                candidates: eviction_candidates(
                                    dataset.active(),
                                    3,
                                    archived.last_confirmed_at,
                                ),
                            },
                            false,
                        ));
                    }
                    *capture.lock().expect("restore event slot poisoned") = vec![archived.clone()];
                    dataset.archive_mut().retain(|entry| entry.id != id);
                    dataset.active_mut().push(archived);
                    Ok((RestoreResult::Restored { id }, true))
                })
                .await?;
            if matches!(outcome, RestoreResult::Restored { .. }) {
                // 同一 op，stage="restore" 与 archive 区分；affected=恢复条目全文。
                let affected = slot.lock().expect("restore event slot poisoned").clone();
                let mut event = self.build_write_event(
                    MemoryEventOp::ArchiveRestore,
                    correlation.clone(),
                    correlation,
                    EventChange::Lifecycle {
                        stage: "restore".to_string(),
                        affected,
                    },
                );
                event.layer = Some(layer);
                self.emit_event(event).await;
                return Ok(outcome);
            }
            if let RestoreResult::NeedsEviction { candidates } = &outcome {
                let affected: Vec<MemoryEntry> = candidates
                    .iter()
                    .map(|candidate| candidate.entry.clone())
                    .collect();
                let correlation = format!("eviction_watermark-restore-{}", uuid::Uuid::now_v7());
                let mut event = self.build_lifecycle_event(
                    MemoryEventOp::EvictionWatermark,
                    "eviction_watermark",
                    correlation,
                    EventActor::Service,
                );
                event.change = EventChange::Lifecycle {
                    stage: "eviction_watermark".to_string(),
                    affected,
                };
                event.context.trigger_summary =
                    Some(format!("path=restore;candidates={}", candidates.len()));
                self.emit_event(event).await;
                return Ok(outcome);
            }
            if !matches!(outcome, RestoreResult::NotFound) {
                return Ok(outcome);
            }
        }
        Ok(RestoreResult::NotFound)
    }

    async fn compact(&self) -> Result<CompactResult, MemoryError> {
        // Compact spans both layers, but each layer is committed as its own
        // observable mutation. A single layer failing surfaces the real error;
        // no partial commit is hidden behind one aggregate result.
        // 一次逻辑 compact 共用一条 correlation；每层 commit 各 emit 一条。
        let correlation = format!("compact-{}", uuid::Uuid::now_v7());
        let mut archived = 0;
        let mut remaining = 0;
        for layer in [MemoryLayer::Global, MemoryLayer::Project] {
            let CompactResult {
                archived: layer_archived,
                remaining: layer_remaining,
            } = self.compact_layer(layer, &correlation).await?;
            archived += layer_archived;
            remaining += layer_remaining;
        }
        Ok(CompactResult {
            archived,
            remaining,
        })
    }

    async fn list(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry> {
        let (global, project) = self.load_latest_layers().await;
        let entries: Vec<MemoryEntry> = global
            .active()
            .iter()
            .chain(project.active())
            .filter(|entry| layer.is_none_or(|layer| entry.layer == layer))
            .cloned()
            .collect();
        let correlation = format!("list_stats-list-{}", (self.clock)());
        self.emit_event(self.build_read_event(
            MemoryEventOp::ListStats,
            correlation.clone(),
            correlation,
            EventChange::Read {
                candidates: entries.clone(),
                hit_count: entries.len() as u32,
                limit: entries.len() as u32,
                layer_filter: layer,
            },
            EventContext {
                trigger_summary: Some("kind=list".to_string()),
                ..EventContext::default()
            },
        ))
        .await;
        entries
    }

    async fn stats(&self) -> MemoryStats {
        let (global, project) = self.load_latest_layers().await;
        let stats = MemoryStats {
            global_count: global.active().len(),
            global_archive_count: global.archive().len(),
            project_count: project.active().len(),
            project_archive_count: project.archive().len(),
        };
        let hit_count = (stats.global_count
            + stats.global_archive_count
            + stats.project_count
            + stats.project_archive_count) as u32;
        let correlation = format!("list_stats-stats-{}", (self.clock)());
        self.emit_event(self.build_read_event(
            MemoryEventOp::ListStats,
            correlation.clone(),
            correlation,
            EventChange::Read {
                candidates: Vec::new(),
                hit_count,
                limit: 0,
                layer_filter: None,
            },
            EventContext {
                trigger_summary: Some(format!(
                    "kind=stats;global={};global_archive={};project={};project_archive={}",
                    stats.global_count,
                    stats.global_archive_count,
                    stats.project_count,
                    stats.project_archive_count
                )),
                ..EventContext::default()
            },
        ))
        .await;
        stats
    }

    fn event_append_port(&self) -> Option<Arc<dyn MemoryEventAppendPort>> {
        Some(Arc::clone(&self.events))
    }
}

fn reflection_memory_id(_now: u64) -> Result<MemoryId, MemoryError> {
    Ok(MemoryId::now_v7())
}

fn reflection_capacity_error() -> MemoryError {
    MemoryError::InvalidEntry {
        message: "Reflection 淘汰非 pinned 候选后重试一次仍超过记忆容量".to_string(),
    }
}

fn partial_apply_or(error: MemoryError, result: &ReflectionApplyResult) -> MemoryError {
    if result.completed == 0 {
        error
    } else {
        MemoryError::PartialApply {
            result_attempted: result.attempted,
            result_completed: result.completed,
            suggestions_added: result.suggestions_added,
            outdated_marked: result.outdated_marked,
            superseded: result.superseded,
        }
    }
}

/// Applies one Reflection suggestion as one dataset mutation. On capacity it
/// archives non-pinned candidates and retries the insertion exactly once.
fn apply_reflection_entry(
    dataset: &mut MemoryDataset,
    entry: &MemoryEntry,
    policy: MemoryPolicy,
) -> Result<(WriteResult, bool), MemoryError> {
    validate_content(&entry.content)?;
    // M11: a synthesized entry may only cite memories this layer already
    // holds — an untraceable conclusion is worse than no conclusion.
    if entry.evidence.iter().any(|source| {
        !dataset
            .active()
            .iter()
            .chain(dataset.archive())
            .any(|stored| &stored.id == source)
    }) {
        return Err(MemoryError::InvalidEntry {
            message: "证据指针指向不存在的记忆".to_string(),
        });
    }
    if dataset
        .active()
        .iter()
        .chain(dataset.archive())
        .any(|stored| stored.id == entry.id)
    {
        return Err(MemoryError::InvalidEntry {
            message: "记忆 ID 必须唯一".to_string(),
        });
    }
    let dedup_hit = dataset
        .active_mut()
        .iter_mut()
        .find(|stored| {
            jaccard_similarity(&stored.content, &entry.content) >= policy.similarity_threshold
        })
        .map(|existing| {
            let mut tags = entry.tags.clone();
            existing.tags.append(&mut tags);
            existing.tags.sort();
            existing.tags.dedup();
            existing.last_confirmed_at = entry.created_at;
            existing.confirmation_count = existing.confirmation_count.saturating_add(1);
            existing.id
        });
    if let Some(existing_id) = dedup_hit {
        // #1775: archive the incoming entry and let the survivor point at it.
        let incoming_id = entry.id;
        dataset.archive_mut().push(entry.clone());
        if let Some(existing) = dataset
            .active_mut()
            .iter_mut()
            .find(|stored| stored.id == existing_id)
        {
            existing.evidence.push(incoming_id);
        }
        return Ok((WriteResult::Merged { existing_id }, true));
    }
    if dataset.active().len() >= policy.max_entries {
        let candidates = eviction_candidates(dataset.active(), 3, entry.created_at);
        let ids = candidates
            .iter()
            .map(|candidate| candidate.entry.id)
            .collect::<Vec<_>>();
        let mut moved = Vec::new();
        dataset.active_mut().retain(|stored| {
            if ids.contains(&stored.id) && !stored.pinned {
                moved.push(stored.clone());
                false
            } else {
                true
            }
        });
        dataset.archive_mut().extend(moved);
        if dataset.active().len() >= policy.max_entries {
            return Err(reflection_capacity_error());
        }
    }
    let id = entry.id;
    dataset.active_mut().push(entry.clone());
    Ok((WriteResult::Added { id }, true))
}

fn validate_policy(policy: MemoryPolicy) -> Result<(), MemoryError> {
    if policy.max_entries == 0 || !(0.0..=1.0).contains(&policy.similarity_threshold) {
        return Err(MemoryError::InvalidEntry {
            message: "无效的记忆策略".to_string(),
        });
    }
    Ok(())
}

/// Loads one layer's committed generation and enforces that Storage returned a
/// dataset for the requested layer.
async fn load_layer<S: MemoryDatasetStore>(
    store: &S,
    layer: MemoryLayer,
) -> Result<LayerState<S::Revision>, MemoryError> {
    let loaded = store.load_committed(layer).await?;
    if loaded.dataset.layer() != layer {
        return Err(MemoryError::Storage {
            kind: MemoryStorageErrorKind::Serialization,
        });
    }
    Ok(LayerState {
        dataset: loaded.dataset,
        revision: loaded.revision,
    })
}

fn is_concurrent_write(error: &MemoryError) -> bool {
    matches!(
        error,
        MemoryError::Storage {
            kind: MemoryStorageErrorKind::ConcurrentWrite
        }
    )
}

/// 在单层数据集中按 id 查找条目全文（active 优先，其次归档）。
fn lookup_entry(dataset: &MemoryDataset, id: &MemoryId) -> Option<MemoryEntry> {
    dataset
        .active()
        .iter()
        .chain(dataset.archive())
        .find(|entry| &entry.id == id)
        .cloned()
}

/// 在 (global, project) 提交态快照中按 id 查找条目全文。
fn lookup_snapshot(
    snapshot: &(MemoryDataset, MemoryDataset),
    id: &MemoryId,
) -> Option<MemoryEntry> {
    lookup_entry(&snapshot.0, id).or_else(|| lookup_entry(&snapshot.1, id))
}

fn mutate_active(
    dataset: &mut MemoryDataset,
    id: &MemoryId,
    mutation: impl FnOnce(&mut MemoryEntry),
) -> bool {
    if let Some(entry) = dataset
        .active_mut()
        .iter_mut()
        .find(|entry| &entry.id == id)
    {
        mutation(entry);
        true
    } else {
        false
    }
}

fn validate_content(content: &str) -> Result<(), MemoryError> {
    if content.trim().is_empty() {
        return Err(MemoryError::InvalidEntry {
            message: "记忆内容不能为空".to_string(),
        });
    }
    Ok(())
}

fn matches_filters(
    entry: &MemoryEntry,
    layer: Option<MemoryLayer>,
    category: Option<MemoryCategory>,
) -> bool {
    layer.is_none_or(|layer| entry.layer == layer)
        && category.is_none_or(|category| entry.category == category)
}

#[cfg(test)]
#[path = "service_reflection_tests.rs"]
mod reflection_tests;

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
