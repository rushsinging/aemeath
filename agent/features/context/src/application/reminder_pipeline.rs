//! Reminder 注入管线（07-reminder-pipeline.md）：Run 级队列 + 注入决策。
//!
//! #2146 数据化接口 + 方案 A 生命周期契约：事实数据驻留 session 级
//! [`ReminderFactStore`]（跨 Run 存活、显式失效），本管线只持 Run 级
//! 调度队列，按事实策略的 refresh trigger 在对应时机从 store 取数入队；
//! 渲染模板由 Context 按 kind 内置（`render_invocation_reminder_body`）。
//! 原 `ReminderSource` trait 多态形态退役。

use crate::domain::reminder::{
    compose_reminder_envelope, compose_tail_user_message, render_invocation_reminder_body,
    CompactBehavior, InjectionCandidate, RefreshTrigger, ReminderEnqueue, ReminderEnvelopeInput,
    ReminderEventSource, ReminderFactData, ReminderFactStore, ReminderKind, ReminderPlacement,
    ReminderQueue, ReminderSnapshot,
};
use crate::domain::{estimate_tokens, ContextMessage, InvocationReminderData, SystemBlock};
use crate::LOG_TARGET;
use share::message::ContentBlock;

/// 一次 build_window 的 reminder 注入产物：尾部合并消息 + SystemTail 块。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReminderWindowInjection {
    /// 合并为单条的尾部 pending user message（多个 `<system-reminder>` 块）。
    pub tail_user_message: Option<String>,
    /// Run 级恒定 reminder 的 system block 尾部追加块（cacheable、无 cache_break）。
    pub system_tail_blocks: Vec<SystemBlock>,
}

/// Run 级 reminder 管线：事实推送 + 时机入队 + 注入组装。
///
/// 生命周期由 ContextApplicationService 管理（Run 启动创建、结束销毁）；
/// 时机入口（run_started / handle_event / task_mutated / step_advanced /
/// compact_committed）由 Runtime 推送，注入入口由 build_window 调用。
pub struct ReminderPipeline {
    queue: ReminderQueue,
    /// 本 Run 允许的 kind 白名单（create 时注册）。
    allowed: Vec<ReminderKind>,
    /// 本 Run 已注入、待 step 收口落盘的尾部 reminder 消息（#1848）：
    /// append_finalized 时取走并插入提交消息头部（u 之后 a 之前），
    /// 使注入轮后的请求前缀逐 token 稳定（prompt cache 命中）。
    pending_persist: Vec<ContextMessage>,
}

impl ReminderPipeline {
    pub fn new(allowed: Vec<ReminderKind>) -> Self {
        Self {
            queue: ReminderQueue::new(),
            allowed,
            pending_persist: Vec::new(),
        }
    }

    /// 该管线允许的 kind（白名单）。
    pub fn allows(&self, kind: &ReminderKind) -> bool {
        self.allowed.contains(kind)
    }

    /// 推送 OneShot 事实（事件型）：直接入队，不入 store（消费即一次性）。
    /// 白名单/自洽性由调用方（service）校验；此处再守一次 allowed。
    pub fn push_oneshot(&mut self, fact: ReminderFactData) {
        if !self.allowed.contains(&fact.kind) {
            log::warn!(
                target: LOG_TARGET,
                "reminder_fact_rejected reason=kind_not_allowed kind={}",
                fact.kind.as_str(),
            );
            return;
        }
        self.enqueue_fact(&fact);
    }

    /// 显式失效：丢弃该 kind 的待注入条目与指纹（保持队列与 store 一致）。
    pub fn discard_pending(&mut self, kind: &ReminderKind) {
        self.queue.discard(kind);
    }

    /// Run 收口：取走仍未注入的 OneShot 回滚标识（供 Runtime 撤销交付标记）。
    pub fn drain_uninjected_oneshot_keys(&mut self) -> Vec<String> {
        self.queue.drain_uninjected_oneshot_keys()
    }

    /// Run 启动：`OnRunStart` 型缓存事实入队；`OnStepInterval` 以 step=0
    /// 推进（0 是任意间隔的倍数——「启动即一次 + 周期重注入」无需双声明）。
    pub fn run_started(&mut self, store: &ReminderFactStore) {
        self.enqueue_matching(store, "run_started", |trigger| {
            matches!(trigger, RefreshTrigger::OnRunStart)
        });
        self.enqueue_interval(store, 0);
    }

    /// Runtime 推送事件：`OnEvent(source)` 匹配的缓存事实入队
    /// （事件累积语义——若推送时 refresh 不匹配此事件，此处兜底）。
    pub fn handle_event(&mut self, store: &ReminderFactStore, event_source: &ReminderEventSource) {
        self.enqueue_matching(
            store,
            "event",
            |trigger| matches!(trigger, RefreshTrigger::OnEvent(owned) if owned == event_source),
        );
    }

    /// task store 变更后：`OnTaskMutation` 型缓存事实入队。
    pub fn task_mutated(&mut self, store: &ReminderFactStore) {
        self.enqueue_matching(store, "task_mutated", |trigger| {
            matches!(trigger, RefreshTrigger::OnTaskMutation)
        });
    }

    /// 用户消息到达：`OnUserMessage` 型缓存事实入队（快照替换语义）。
    pub fn user_message_received(&mut self, store: &ReminderFactStore) {
        self.enqueue_matching(store, "user_message", |trigger| {
            matches!(trigger, RefreshTrigger::OnUserMessage)
        });
    }

    /// step 边界推进：`OnStepInterval(n)` 在 step 为 n 的倍数时入队。
    pub fn step_advanced(&mut self, store: &ReminderFactStore, step: u64) {
        self.enqueue_interval(store, step);
    }

    fn enqueue_interval(&mut self, store: &ReminderFactStore, step: u64) {
        self.enqueue_matching(store, "step_interval", |trigger| {
            matches!(trigger, RefreshTrigger::OnStepInterval(interval)
                if *interval > 0 && step.is_multiple_of(u64::from(*interval)))
        });
    }

    /// auto-compact `Committed`：按 per-kind compact 处置分发（Skipped 不调用）。
    pub fn compact_committed(&mut self, store: &ReminderFactStore) {
        let mut outcomes: Vec<(ReminderKind, CompactBehavior)> = store
            .facts()
            .map(|fact| (fact.kind.clone(), fact.policy.compact))
            .collect();
        // 队列内仍待注入的事件型事实（OneShot 不入 store）同样需要处置。
        for entry in self.queue.pending_entries() {
            if !outcomes.iter().any(|(kind, _)| *kind == entry.kind) {
                outcomes.push((entry.kind.clone(), entry.policy.compact));
            }
        }
        self.queue.apply_compact_outcome(&outcomes);
    }

    /// 注入决策：rebuild 标记重建 → drain 候选 → 渲染 envelope →
    /// 预算截断滞留 → 按 placement 分组产出。
    pub fn inject_into_window(
        &mut self,
        store: &ReminderFactStore,
        language: &str,
        at: &str,
        token_budget: usize,
    ) -> ReminderWindowInjection {
        self.rebuild_flagged_facts(store);
        let candidates = self.queue.begin_injection();
        if candidates.is_empty() {
            return ReminderWindowInjection::default();
        }

        let mut tail_blocks = Vec::new();
        let mut tail_persist_blocks: Vec<ContentBlock> = Vec::new();
        let mut system_blocks = Vec::new();
        let mut injected_seqs = Vec::new();
        let mut deferred_seqs = Vec::new();
        let mut budget_used = 0usize;

        for candidate in &candidates {
            let placement = candidate.policy.placement;
            let (envelope, persist_block) = render_block(candidate, placement, language, at);
            let block_tokens = estimate_tokens(&envelope);
            if budget_used + block_tokens > token_budget {
                deferred_seqs.push(candidate.seq);
                log::debug!(
                    target: LOG_TARGET,
                    "reminder_deferred kind={} seq={} tokens={} budget={}",
                    candidate.kind.as_str(),
                    candidate.seq,
                    block_tokens,
                    token_budget,
                );
                continue;
            }
            budget_used += block_tokens;
            injected_seqs.push(candidate.seq);
            log::debug!(
                target: LOG_TARGET,
                "reminder_injected kind={} seq={} placement={:?} tokens={}",
                candidate.kind.as_str(),
                candidate.seq,
                placement,
                block_tokens,
            );
            match placement {
                ReminderPlacement::TailUserMessage => {
                    tail_blocks.push(envelope);
                    if let Some(block) = persist_block {
                        tail_persist_blocks.push(block);
                    }
                }
                ReminderPlacement::SystemTail => system_blocks.push(envelope),
            }
        }

        self.queue.confirm_injection(&injected_seqs);
        self.queue.defer_injection(&deferred_seqs);

        if !tail_persist_blocks.is_empty() {
            // typed Reminder 落盘：TUI 按 type=reminder 隐藏；LLM 出站走 to_llm_view envelope。
            self.pending_persist
                .push(ContextMessage::reminder(tail_persist_blocks));
        }

        ReminderWindowInjection {
            tail_user_message: (!tail_blocks.is_empty())
                .then(|| compose_tail_user_message(&tail_blocks)),
            system_tail_blocks: system_blocks
                .into_iter()
                .map(|content| SystemBlock {
                    kind: "reminder".to_string(),
                    content,
                    cacheable: true,
                    cache_break: false,
                })
                .collect(),
        }
    }

    /// 取走待落盘的注入消息（append_finalized 提交前调用；一次后清空）。
    pub fn take_pending_persist_messages(&mut self) -> Vec<ContextMessage> {
        std::mem::take(&mut self.pending_persist)
    }

    // ─── 内部 ───

    fn enqueue_matching(
        &mut self,
        store: &ReminderFactStore,
        trigger_label: &str,
        predicate: impl Fn(&RefreshTrigger) -> bool,
    ) {
        let matched: Vec<ReminderFactData> = store
            .facts()
            .filter(|fact| predicate(&fact.policy.refresh))
            .filter(|fact| self.allowed.contains(&fact.kind))
            .cloned()
            .collect();
        for fact in matched {
            self.enqueue_fact(&fact);
            log::debug!(
                target: LOG_TARGET,
                "reminder_enqueued trigger={trigger_label} kind={}",
                fact.kind.as_str(),
            );
        }
    }

    fn enqueue_fact(&mut self, fact: &ReminderFactData) {
        let snapshot = ReminderSnapshot {
            data: fact.data.clone(),
        };
        match fact.enqueue {
            ReminderEnqueue::Event => self.queue.push_event_with_rollback(
                fact.kind.clone(),
                snapshot,
                fact.policy.clone(),
                fact.rollback_keys.clone(),
            ),
            ReminderEnqueue::Snapshot => self.queue.push_snapshot_with_rollback(
                fact.kind.clone(),
                snapshot,
                fact.policy.clone(),
                fact.rollback_keys.clone(),
            ),
        }
    }

    fn rebuild_flagged_facts(&mut self, store: &ReminderFactStore) {
        let flagged: Vec<ReminderKind> = store
            .facts()
            .map(|fact| fact.kind.clone())
            .filter(|kind| self.queue.needs_rebuild(kind))
            .collect();
        for kind in flagged {
            if let Some(fact) = store.get(&kind) {
                self.enqueue_fact(fact);
            }
            self.queue.clear_rebuild_flag(&kind);
        }
    }
}

/// 渲染单条候选（kind 内置模板；无 source 多态）：
/// 返回 (LLM envelope 文本, TailUserMessage 落盘用 Reminder block)。
fn render_block(
    candidate: &InjectionCandidate,
    placement: ReminderPlacement,
    language: &str,
    at: &str,
) -> (String, Option<ContentBlock>) {
    let body = match serde_json::from_str::<InvocationReminderData>(&candidate.snapshot.data) {
        Ok(data) => render_invocation_reminder_body(&data, language),
        Err(error) => {
            log::warn!(
                target: LOG_TARGET,
                "reminder 快照反序列化失败 kind={} error={error}",
                candidate.kind.as_str()
            );
            String::new()
        }
    };
    let kind_wire = candidate.kind.as_str().replace('_', "-");
    let envelope = compose_reminder_envelope(&ReminderEnvelopeInput {
        kind: candidate.kind.as_str().to_string(),
        body: body.clone(),
        at: at.to_string(),
        seq: candidate.seq,
    });
    let persist = matches!(placement, ReminderPlacement::TailUserMessage)
        .then(|| ContentBlock::reminder(kind_wire, 1, at, candidate.seq, body));
    (envelope, persist)
}
