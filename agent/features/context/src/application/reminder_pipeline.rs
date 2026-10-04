//! Reminder 注入调度（application 层）：事件触发入队、compact 处置与
//! invocation 边界注入决策。领域规则在 `domain/reminder.rs`；
//! 本模块只做编排（时间戳与预算由调用方提供）。

use std::sync::Arc;

use crate::domain::reminder::{
    compose_reminder_envelope, compose_tail_user_message, InjectionCandidate, RefreshTrigger,
    ReminderEnvelopeInput, ReminderEventSource, ReminderKind, ReminderPlacement, ReminderPolicy,
    ReminderQueue, ReminderSource,
};
use crate::domain::{estimate_tokens, SystemBlock};

/// 一次 build_window 的 reminder 注入产物：尾部合并消息 + SystemTail 块。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReminderWindowInjection {
    /// 合并为单条的尾部 pending user message（多个 `<system-reminder>` 块）。
    pub tail_user_message: Option<String>,
    /// Run 级恒定 reminder 的 system block 尾部追加块（cacheable、无 cache_break）。
    pub system_tail_blocks: Vec<SystemBlock>,
}

/// Run-scoped reminder 管线句柄：注册 sources + Run 级队列。
///
/// 生命周期由 ContextApplicationService 管理（Run 启动创建、结束销毁）；
/// 事件入口（run_started / handle_event / task_mutated / step_advanced /
/// compact_committed）由 Runtime 推送，注入入口由 build_window 调用。
pub struct ReminderPipeline {
    queue: ReminderQueue,
    sources: Vec<Arc<dyn ReminderSource>>,
}

impl ReminderPipeline {
    pub fn new(sources: Vec<Arc<dyn ReminderSource>>) -> Self {
        Self {
            queue: ReminderQueue::new(),
            sources,
        }
    }

    /// Run 启动：`OnRunStart` 类 source 现场快照入队（快照替换语义）；
    /// `OnStepInterval` 以 step=0 推进（0 是任意间隔的倍数——
    /// 「启动即一次 + 周期重注入」的 kind 无需双声明）。
    pub fn run_started(&mut self) {
        for source in self.sources_matching(|trigger| matches!(trigger, RefreshTrigger::OnRunStart))
        {
            if let Some(snapshot) = source.build() {
                self.queue
                    .push_snapshot(source.kind(), snapshot, source.policy().inject);
            }
            log::debug!(
                target: crate::LOG_TARGET,
                "reminder_enqueued trigger=run_started kind={}",
                source.kind().as_str(),
            );
        }
        self.enqueue_interval_sources(0);
    }

    /// Runtime 推送事件：`OnEvent(source)` 匹配的 source 入队（事件累积语义）。
    pub fn handle_event(&mut self, event_source: &ReminderEventSource) {
        for source in self.sources_matching(
            |trigger| matches!(trigger, RefreshTrigger::OnEvent(owned) if owned == event_source),
        ) {
            if let Some(snapshot) = source.build() {
                self.queue
                    .push_event(source.kind(), snapshot, source.policy().inject);
            }
            log::debug!(
                target: crate::LOG_TARGET,
                "reminder_enqueued trigger=event source={} kind={}",
                event_source.as_str(),
                source.kind().as_str(),
            );
        }
    }

    /// task store 变更后：`OnTaskMutation` 类 source 入队。
    pub fn task_mutated(&mut self) {
        for source in
            self.sources_matching(|trigger| matches!(trigger, RefreshTrigger::OnTaskMutation))
        {
            if let Some(snapshot) = source.build() {
                self.queue
                    .push_snapshot(source.kind(), snapshot, source.policy().inject);
            }
        }
    }

    /// step 边界推进：`OnStepInterval(n)` 在 step 为 n 的倍数时现场重建入队。
    pub fn step_advanced(&mut self, step: u64) {
        self.enqueue_interval_sources(step);
    }

    fn enqueue_interval_sources(&mut self, step: u64) {
        for source in self.sources_matching(|trigger| {
            matches!(trigger, RefreshTrigger::OnStepInterval(interval)
                if *interval > 0 && step.is_multiple_of(u64::from(*interval)))
        }) {
            if let Some(snapshot) = source.build() {
                self.queue
                    .push_snapshot(source.kind(), snapshot, source.policy().inject);
                log::debug!(
                    target: crate::LOG_TARGET,
                    "reminder_enqueued trigger=step_interval step={step} kind={}",
                    source.kind().as_str(),
                );
            }
        }
    }

    /// auto-compact `Committed`：按 per-kind compact 处置分发（Skipped 不调用）。
    pub fn compact_committed(&mut self) {
        let outcomes = self
            .sources
            .iter()
            .map(|source| (source.kind(), source.policy().compact))
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
        self.rebuild_flagged_sources();
        let candidates = self.queue.begin_injection();
        if candidates.is_empty() {
            return ReminderWindowInjection::default();
        }

        let mut tail_blocks = Vec::new();
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
            let block = self.render_block(candidate, placement, language, at);
            let block_tokens = estimate_tokens(&block);
            if budget_used + block_tokens > token_budget {
                deferred_seqs.push(candidate.seq);
                log::debug!(
                    target: crate::LOG_TARGET,
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
                target: crate::LOG_TARGET,
                "reminder_injected kind={} seq={} placement={:?} tokens={}",
                candidate.kind.as_str(),
                candidate.seq,
                placement,
                block_tokens,
            );
            match placement {
                ReminderPlacement::TailUserMessage => tail_blocks.push(block),
                ReminderPlacement::SystemTail => system_blocks.push(block),
            }
        }

        self.queue.confirm_injection(&injected_seqs);
        self.queue.defer_injection(&deferred_seqs);

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

    fn rebuild_flagged_sources(&mut self) {
        let flagged = self
            .sources
            .iter()
            .filter(|source| self.queue.needs_rebuild(&source.kind()))
            .map(|source| source.kind())
            .collect::<Vec<_>>();
        for kind in flagged {
            if let Some(source) = self.source_for_kind(&kind) {
                if let Some(snapshot) = source.build() {
                    self.queue
                        .push_snapshot(kind.clone(), snapshot, source.policy().inject);
                }
            }
            self.queue.clear_rebuild_flag(&kind);
        }
    }

    fn render_block(
        &self,
        candidate: &InjectionCandidate,
        placement: ReminderPlacement,
        language: &str,
        at: &str,
    ) -> String {
        let body = match placement {
            ReminderPlacement::TailUserMessage | ReminderPlacement::SystemTail => self
                .source_for_kind(&candidate.kind)
                .map(|source| source.render(&candidate.snapshot, language))
                .unwrap_or_default(),
        };
        compose_reminder_envelope(&ReminderEnvelopeInput {
            kind: candidate.kind.as_str().to_string(),
            body,
            at: at.to_string(),
            seq: candidate.seq,
        })
    }

    fn source_for_kind(&self, kind: &ReminderKind) -> Option<&Arc<dyn ReminderSource>> {
        self.sources.iter().find(|source| &source.kind() == kind)
    }

    fn policies_by_kind(&self) -> std::collections::HashMap<ReminderKind, ReminderPolicy> {
        self.sources
            .iter()
            .map(|source| (source.kind(), source.policy()))
            .collect()
    }

    fn sources_matching(
        &self,
        predicate: impl Fn(&RefreshTrigger) -> bool,
    ) -> Vec<Arc<dyn ReminderSource>> {
        self.sources
            .iter()
            .filter(|source| predicate(&source.policy().refresh))
            .cloned()
            .collect()
    }
}
