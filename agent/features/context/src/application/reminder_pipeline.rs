//! Reminder 注入管线（07-reminder-pipeline.md）：Run 级队列 + 注入决策。
//!
//! #2146 数据化接口：Runtime 经 [`ReminderPipeline::push_fact`] 推送
//! [`ReminderFactData`]（kind + 策略 + 渲染载荷），本管线按策略的
//! refresh trigger 在对应时机入队；渲染模板由 Context 按 kind 内置
//! （`render_invocation_reminder_body`）。原 `ReminderSource` trait
//! 多态形态退役（数据在 Runtime、管线在 Context 的缝改经数据推送）。

use std::collections::HashMap;

use crate::domain::reminder::{
    compose_reminder_envelope, compose_tail_user_message, render_invocation_reminder_body,
    InjectionCandidate, RefreshTrigger, ReminderEnqueue, ReminderEnvelopeInput,
    ReminderEventSource, ReminderFactData, ReminderKind, ReminderPlacement, ReminderPolicy,
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
    /// 各 kind 最新推送的事实（数据缓存：时机到来时入队）。
    facts: HashMap<ReminderKind, ReminderFactData>,
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
            facts: HashMap::new(),
            pending_persist: Vec::new(),
        }
    }

    /// Runtime 推送一条事实（数据入口，#2146）：更新缓存；入队统一由
    /// 时机方法驱动（run_started / step_advanced / handle_event /
    /// task_mutated / user_message_received 按 policy.refresh 匹配）。
    /// 非白名单 kind 拒收（warn）；策略合法性在此校验。
    pub fn push_fact(&mut self, fact: ReminderFactData) {
        if !self.allowed.contains(&fact.kind) {
            log::warn!(
                target: LOG_TARGET,
                "reminder_fact_rejected reason=kind_not_allowed kind={}",
                fact.kind.as_str(),
            );
            return;
        }
        if !fact.policy.is_valid() {
            log::warn!(
                target: LOG_TARGET,
                "reminder_fact_rejected reason=dynamic_refresh_with_system_tail kind={}",
                fact.kind.as_str(),
            );
            return;
        }
        let kind = fact.kind.clone();
        self.facts.insert(kind, fact);
    }

    /// Run 启动：`OnRunStart` 型缓存事实入队；`OnStepInterval` 以 step=0
    /// 推进（0 是任意间隔的倍数——「启动即一次 + 周期重注入」无需双声明）。
    pub fn run_started(&mut self) {
        self.enqueue_matching("run_started", |trigger| {
            matches!(trigger, RefreshTrigger::OnRunStart)
        });
        self.enqueue_interval(0);
    }

    /// Runtime 推送事件：`OnEvent(source)` 匹配的缓存事实入队
    /// （事件累积语义——若推送时 refresh 不匹配此事件，此处兜底）。
    pub fn handle_event(&mut self, event_source: &ReminderEventSource) {
        self.enqueue_matching(
            "event",
            |trigger| matches!(trigger, RefreshTrigger::OnEvent(owned) if owned == event_source),
        );
    }

    /// task store 变更后：`OnTaskMutation` 型缓存事实入队。
    pub fn task_mutated(&mut self) {
        self.enqueue_matching("task_mutated", |trigger| {
            matches!(trigger, RefreshTrigger::OnTaskMutation)
        });
    }

    /// 用户消息到达：`OnUserMessage` 型缓存事实入队（快照替换语义）。
    pub fn user_message_received(&mut self) {
        self.enqueue_matching("user_message", |trigger| {
            matches!(trigger, RefreshTrigger::OnUserMessage)
        });
    }

    /// step 边界推进：`OnStepInterval(n)` 在 step 为 n 的倍数时入队。
    pub fn step_advanced(&mut self, step: u64) {
        self.enqueue_interval(step);
    }

    fn enqueue_interval(&mut self, step: u64) {
        self.enqueue_matching("step_interval", |trigger| {
            matches!(trigger, RefreshTrigger::OnStepInterval(interval)
                if *interval > 0 && step.is_multiple_of(u64::from(*interval)))
        });
    }

    /// auto-compact `Committed`：按 per-kind compact 处置分发（Skipped 不调用）。
    pub fn compact_committed(&mut self) {
        let outcomes = self
            .facts
            .values()
            .map(|fact| (fact.kind.clone(), fact.policy.compact))
            .collect::<Vec<_>>();
        self.queue.apply_compact_outcome(&outcomes);
    }

    /// 注入决策：rebuild 标记重建 → drain 候选 → 渲染 envelope →
    /// 预算截断滞留 → 按 placement 分组产出。
    pub fn inject_into_window(
        &mut self,
        language: &str,
        at: &str,
        token_budget: usize,
    ) -> ReminderWindowInjection {
        self.rebuild_flagged_facts();
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
        let policies = self.policies_by_kind();

        for candidate in &candidates {
            let placement = policies
                .get(&candidate.kind)
                .map(|policy| policy.placement)
                .unwrap_or(ReminderPlacement::TailUserMessage);
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
        trigger_label: &str,
        predicate: impl Fn(&RefreshTrigger) -> bool,
    ) {
        let matched: Vec<ReminderKind> = self
            .facts
            .values()
            .filter(|fact| predicate(&fact.policy.refresh))
            .map(|fact| fact.kind.clone())
            .collect();
        for kind in matched {
            if let Some(fact) = self.facts.get(&kind) {
                let snapshot = ReminderSnapshot {
                    data: fact.data.clone(),
                };
                match fact.enqueue {
                    ReminderEnqueue::Event => {
                        self.queue
                            .push_event(kind.clone(), snapshot, fact.policy.inject)
                    }
                    ReminderEnqueue::Snapshot => {
                        self.queue
                            .push_snapshot(kind.clone(), snapshot, fact.policy.inject)
                    }
                }
                log::debug!(
                    target: LOG_TARGET,
                    "reminder_enqueued trigger={trigger_label} kind={}",
                    kind.as_str(),
                );
            }
        }
    }

    fn rebuild_flagged_facts(&mut self) {
        let flagged: Vec<ReminderKind> = self
            .facts
            .keys()
            .filter(|kind| self.queue.needs_rebuild(kind))
            .cloned()
            .collect();
        for kind in flagged {
            if let Some(fact) = self.facts.get(&kind) {
                let snapshot = ReminderSnapshot {
                    data: fact.data.clone(),
                };
                self.queue
                    .push_snapshot(kind.clone(), snapshot, fact.policy.inject);
            }
            self.queue.clear_rebuild_flag(&kind);
        }
    }

    fn policies_by_kind(&self) -> HashMap<ReminderKind, ReminderPolicy> {
        self.facts
            .values()
            .map(|fact| (fact.kind.clone(), fact.policy.clone()))
            .collect()
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
