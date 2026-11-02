//! Reminder 管线行为测试（#2146 数据化接口：push_fact + 时机方法）。

use super::reminder_pipeline::ReminderPipeline;
use crate::domain::reminder::{
    CompactBehavior, InjectBehavior, RefreshTrigger, ReminderDedup, ReminderEnqueue,
    ReminderEventSource, ReminderFactData, ReminderFactLifetime, ReminderFactStore, ReminderKind,
    ReminderPlacement, ReminderPolicy, ReminderPriority,
};

const LANGUAGE_ZH: &str = "zh";
const AT: &str = "2026-10-04T01:00:00+08:00";

fn inject_behavior(priority: ReminderPriority) -> InjectBehavior {
    InjectBehavior {
        dedup: ReminderDedup::SkipIfUnchanged,
        priority,
    }
}

fn interval_policy(interval: u32, placement: ReminderPlacement) -> ReminderPolicy {
    ReminderPolicy {
        refresh: RefreshTrigger::OnStepInterval(interval),
        placement,
        inject: inject_behavior(ReminderPriority::task_state()),
        compact: CompactBehavior::Rebuild,
    }
}

fn event_policy(source: &str) -> ReminderPolicy {
    ReminderPolicy {
        refresh: RefreshTrigger::OnEvent(ReminderEventSource::new(source)),
        placement: ReminderPlacement::TailUserMessage,
        inject: inject_behavior(ReminderPriority::event()),
        compact: CompactBehavior::Drop,
    }
}

fn run_start_policy(placement: ReminderPlacement) -> ReminderPolicy {
    ReminderPolicy {
        refresh: RefreshTrigger::OnRunStart,
        placement,
        inject: inject_behavior(ReminderPriority::environment()),
        compact: CompactBehavior::Reinstate,
    }
}

fn user_message_policy() -> ReminderPolicy {
    ReminderPolicy {
        refresh: RefreshTrigger::OnUserMessage,
        placement: ReminderPlacement::TailUserMessage,
        inject: inject_behavior(ReminderPriority::memory_recall()),
        compact: CompactBehavior::Rebuild,
    }
}

/// 事实载荷：任务进度（渲染出 "total=N completed=M" 形态）。
fn task_progress_fact(
    kind: ReminderKind,
    policy: ReminderPolicy,
    total: usize,
    completed: usize,
) -> ReminderFactData {
    ReminderFactData {
        kind,
        policy,
        enqueue: ReminderEnqueue::Snapshot,
        lifetime: ReminderFactLifetime::Snapshot,
        rollback_keys: Vec::new(),
        data: serde_json::to_string(&crate::domain::InvocationReminderData::TaskProgress(
            crate::domain::TaskProgressReminderData {
                total,
                completed,
                items: Vec::new(),
                hidden_count: 0,
            },
        ))
        .expect("载荷序列化"),
    }
}

/// 通用文本事实（memory_updated 等环境事实）。
fn text_fact(kind: ReminderKind, policy: ReminderPolicy, text: &str) -> ReminderFactData {
    ReminderFactData {
        kind,
        policy,
        enqueue: ReminderEnqueue::Snapshot,
        lifetime: ReminderFactLifetime::Snapshot,
        rollback_keys: Vec::new(),
        data: serde_json::to_string(
            &crate::domain::InvocationReminderData::GuidanceSourcesChanged {
                paths: vec![text.to_string()],
            },
        )
        .expect("载荷序列化"),
    }
}

/// 测试台：session 级 fact store + Run 级管线（模拟 service 分派）。
struct Harness {
    store: ReminderFactStore,
    pipeline: ReminderPipeline,
}

impl Harness {
    fn new(allowed: Vec<ReminderKind>) -> Self {
        Self {
            store: ReminderFactStore::default(),
            pipeline: ReminderPipeline::new(allowed),
        }
    }

    /// 模拟 `push_reminder_fact`：先按 service 同款规则校验，再按生命周期分派
    /// （OneShot 直接入队，其余写 store）。
    fn push(&mut self, fact: ReminderFactData) {
        if !fact.is_valid() || !self.pipeline.allows(&fact.kind) {
            return;
        }
        match fact.lifetime {
            ReminderFactLifetime::OneShot => self.pipeline.push_oneshot(fact),
            _ => self.store.push(fact),
        }
    }

    fn remove(&mut self, kind: &ReminderKind) {
        self.store.remove(kind);
    }

    fn run_started(&mut self) {
        self.pipeline.run_started(&self.store);
    }

    fn step_advanced(&mut self, step: u64) {
        self.pipeline.step_advanced(&self.store, step);
    }

    fn handle_event(&mut self, source: &ReminderEventSource) {
        self.pipeline.handle_event(&self.store, source);
    }

    fn task_mutated(&mut self) {
        self.pipeline.task_mutated(&self.store);
    }

    fn user_message_received(&mut self) {
        self.pipeline.user_message_received(&self.store);
    }

    fn compact_committed(&mut self) {
        self.pipeline.compact_committed(&self.store);
    }

    fn inject(&mut self, budget: usize) -> super::reminder_pipeline::ReminderWindowInjection {
        self.pipeline
            .inject_into_window(&self.store, LANGUAGE_ZH, AT, budget)
    }

    /// 模拟 Run 收口 + 新 Run 创建：store 保留（跨 Run），非 RunScoped 事实存活。
    fn restart_run(&mut self, allowed: Vec<ReminderKind>) {
        self.store.reset_run_scoped(&allowed);
        self.pipeline = ReminderPipeline::new(allowed);
    }

    fn drain_uninjected_oneshot_keys(&mut self) -> Vec<String> {
        self.pipeline.drain_uninjected_oneshot_keys()
    }

    fn store_has(&self, kind: &ReminderKind) -> bool {
        self.store.get(kind).is_some()
    }
}

#[test]
fn run_started_fact_flows_to_tail_user_message() {
    let kind = ReminderKind::task_progress();
    let mut harness = Harness::new(vec![kind.clone()]);
    harness.push(task_progress_fact(
        kind,
        run_start_policy(ReminderPlacement::TailUserMessage),
        3,
        1,
    ));

    harness.run_started();
    let injection = harness.inject(512);

    let tail = injection
        .tail_user_message
        .expect("Run 启动注入尾部 user message");
    assert!(tail.contains("kind=\"task-progress\""), "统一 envelope");
    assert!(tail.contains("version=\"1\""));
    assert!(tail.contains("3"), "渲染 body 含事实数字");
    assert!(injection.system_tail_blocks.is_empty());
}

#[test]
fn event_fact_flows_only_on_matching_event() {
    let kind = ReminderKind::new("memory_updated");
    let mut harness = Harness::new(vec![kind.clone()]);
    harness.push(text_fact(kind, event_policy("memory"), "changed=2"));

    harness.handle_event(&ReminderEventSource::new("background_process"));
    assert!(
        harness.inject(512).tail_user_message.is_none(),
        "非匹配事件源不触发"
    );

    harness.handle_event(&ReminderEventSource::new("memory"));
    let injection = harness.inject(512);
    assert!(injection.tail_user_message.is_some(), "匹配事件源触发注入");
}

#[test]
fn step_interval_enqueues_cached_fact_at_configured_cadence() {
    let kind = ReminderKind::task_progress();
    let mut harness = Harness::new(vec![kind.clone()]);
    harness.push(task_progress_fact(
        kind.clone(),
        interval_policy(3, ReminderPlacement::TailUserMessage),
        3,
        1,
    ));

    harness.step_advanced(1);
    harness.step_advanced(2);
    assert!(
        harness.inject(512).tail_user_message.is_none(),
        "间隔未到不入队"
    );

    harness.push(task_progress_fact(
        kind,
        interval_policy(3, ReminderPlacement::TailUserMessage),
        3,
        2,
    ));
    harness.step_advanced(3);
    let injection = harness.inject(512);
    assert!(
        injection.tail_user_message.is_some(),
        "间隔到达触发注入（内容为最新推送快照）"
    );
}

#[test]
fn system_tail_fact_flows_to_system_blocks() {
    let kind = ReminderKind::new("model_guidance_mismatch");
    let mut harness = Harness::new(vec![kind.clone()]);
    harness.push(text_fact(
        kind,
        run_start_policy(ReminderPlacement::SystemTail),
        "mismatch",
    ));

    harness.run_started();
    let injection = harness.inject(512);

    assert_eq!(injection.system_tail_blocks.len(), 1);
    assert!(injection.tail_user_message.is_none());
    assert!(injection.system_tail_blocks[0]
        .content
        .contains("system-reminder"));
}

#[test]
fn compact_committed_reinstates_run_start_fact() {
    let kind = ReminderKind::task_progress();
    let mut harness = Harness::new(vec![kind.clone()]);
    harness.push(task_progress_fact(
        kind,
        run_start_policy(ReminderPlacement::TailUserMessage),
        3,
        1,
    ));
    harness.run_started();
    let _ = harness.inject(512);

    harness.compact_committed();
    let injection = harness.inject(512);
    assert!(
        injection.tail_user_message.is_some(),
        "Reinstate 型 fact 在 compact 后重注入"
    );
}

#[test]
fn token_budget_defers_block_until_next_round() {
    let low = ReminderKind::new("low_priority");
    let high = ReminderKind::new("high_priority");
    let mut harness = Harness::new(vec![low.clone(), high.clone()]);
    harness.push(text_fact(
        high.clone(),
        run_start_policy(ReminderPlacement::TailUserMessage),
        "high",
    ));
    harness.push(text_fact(
        low.clone(),
        ReminderPolicy {
            inject: inject_behavior(ReminderPriority::environment()),
            ..run_start_policy(ReminderPlacement::TailUserMessage)
        },
        "low",
    ));
    harness.run_started();

    // 极小预算：只容纳一条。
    let first = harness.inject(8);
    let second = harness.inject(4096);
    assert!(first.tail_user_message.is_some() || second.tail_user_message.is_some());
}

#[test]
fn deferred_fact_rebuilds_from_cache_on_next_round() {
    let kind = ReminderKind::task_progress();
    let mut harness = Harness::new(vec![kind.clone()]);
    harness.push(task_progress_fact(
        kind,
        interval_policy(1, ReminderPlacement::TailUserMessage),
        1,
        0,
    ));
    harness.run_started();
    harness.step_advanced(1);

    // 极小预算：本条 defer（滞留），队列置 rebuild 标记。
    let deferred = harness.inject(1);
    assert!(deferred.tail_user_message.is_none(), "预算不足滞留");

    // 下一轮：rebuild 标记从缓存 fact 重建并入队。
    let retried = harness.inject(4096);
    assert!(
        retried.tail_user_message.is_some(),
        "滞留事实下一轮由缓存重建注入"
    );
}

#[test]
fn multiple_facts_merge_into_single_tail_message() {
    let a = ReminderKind::new("kind_a");
    let b = ReminderKind::new("kind_b");
    let mut harness = Harness::new(vec![a.clone(), b.clone()]);
    harness.push(text_fact(
        a,
        run_start_policy(ReminderPlacement::TailUserMessage),
        "a",
    ));
    harness.push(text_fact(
        b,
        run_start_policy(ReminderPlacement::TailUserMessage),
        "b",
    ));
    harness.run_started();

    let tail = harness
        .inject(4096)
        .tail_user_message
        .expect("多条 fact 合并为单条尾部消息");
    assert!(tail.contains("kind=\"kind-a\""));
    assert!(tail.contains("kind=\"kind-b\""));
}

#[test]
fn pipeline_rejects_fact_with_invalid_placement_policy() {
    // 动态 refresh（OnStepInterval）强制 TailUserMessage：SystemTail 组合拒收。
    let kind = ReminderKind::task_progress();
    let mut harness = Harness::new(vec![kind.clone()]);
    harness.push(task_progress_fact(
        kind,
        interval_policy(3, ReminderPlacement::SystemTail),
        3,
        1,
    ));
    harness.run_started();
    harness.step_advanced(3);

    let injection = harness.inject(512);
    assert!(
        injection.tail_user_message.is_none() && injection.system_tail_blocks.is_empty(),
        "非法 placement 组合拒收（不入队）"
    );
}

#[test]
fn pipeline_rejects_fact_with_unregistered_kind() {
    let kind = ReminderKind::task_progress();
    let mut harness = Harness::new(Vec::new());
    harness.push(task_progress_fact(
        kind,
        run_start_policy(ReminderPlacement::TailUserMessage),
        3,
        1,
    ));
    harness.run_started();
    assert!(
        harness.inject(512).tail_user_message.is_none(),
        "非白名单 kind 拒收"
    );
}

#[test]
fn user_message_fact_rebuilds_on_each_message() {
    let kind = ReminderKind::memory_recall();
    let mut harness = Harness::new(vec![kind.clone()]);
    harness.push(text_fact(kind.clone(), user_message_policy(), "first"));
    harness.user_message_received();
    let first = harness.inject(4096);
    assert!(first.tail_user_message.is_some());

    harness.push(text_fact(kind, user_message_policy(), "second"));
    harness.user_message_received();
    let second = harness.inject(4096);
    assert!(second.tail_user_message.is_some(), "每轮用户消息重建注入");
}

#[test]
fn user_message_trigger_leaves_other_triggers_untouched() {
    let run_kind = ReminderKind::task_progress();
    let msg_kind = ReminderKind::memory_recall();
    let mut harness = Harness::new(vec![run_kind.clone(), msg_kind.clone()]);
    harness.push(task_progress_fact(
        run_kind,
        run_start_policy(ReminderPlacement::TailUserMessage),
        1,
        0,
    ));
    harness.push(text_fact(msg_kind, user_message_policy(), "recall"));

    // 只触发 user_message，不触发 run_started：RunStart 型不应入队。
    harness.user_message_received();
    let injection = harness.inject(4096);
    let tail = injection.tail_user_message.expect("user_message 型入队");
    assert!(tail.contains("kind=\"memory-recall\""));
    assert!(
        !tail.contains("kind=\"task-progress\""),
        "其它 trigger 不受影响"
    );
}

#[test]
fn user_message_is_dynamic_and_must_not_use_system_tail() {
    let kind = ReminderKind::memory_recall();
    let mut harness = Harness::new(vec![kind.clone()]);
    let mut policy = user_message_policy();
    policy.placement = ReminderPlacement::SystemTail;
    harness.push(text_fact(kind, policy, "recall"));
    harness.user_message_received();
    let injection = harness.inject(512);
    assert!(
        injection.tail_user_message.is_none() && injection.system_tail_blocks.is_empty(),
        "动态 refresh 配 SystemTail 必须拒收"
    );
}

// ---------- #2146 方案 A：生命周期与失效契约 ----------

fn run_scoped_policy() -> ReminderPolicy {
    ReminderPolicy {
        refresh: RefreshTrigger::OnRunStart,
        placement: ReminderPlacement::TailUserMessage,
        inject: inject_behavior(ReminderPriority::environment()),
        compact: CompactBehavior::Reinstate,
    }
}

fn oneshot_fact(kind: ReminderKind, rollback_key: Option<&str>) -> ReminderFactData {
    ReminderFactData {
        kind,
        policy: event_policy("background_process"),
        enqueue: ReminderEnqueue::Event,
        lifetime: ReminderFactLifetime::OneShot,
        rollback_keys: rollback_key.map(str::to_string).into_iter().collect(),
        data: serde_json::to_string(
            &crate::domain::InvocationReminderData::GuidanceSourcesChanged {
                paths: vec!["bgp_1".to_string()],
            },
        )
        .expect("载荷序列化"),
    }
}

#[test]
fn snapshot_fact_survives_run_restart_but_run_scoped_is_cleared() {
    let snapshot_kind = ReminderKind::task_progress();
    let scoped_kind = ReminderKind::new("model_guidance_mismatch");
    let mut harness = Harness::new(vec![snapshot_kind.clone(), scoped_kind.clone()]);
    harness.push(task_progress_fact(
        snapshot_kind.clone(),
        run_start_policy(ReminderPlacement::TailUserMessage),
        2,
        0,
    ));
    let mut scoped = text_fact(scoped_kind.clone(), run_scoped_policy(), "session=a run=b");
    scoped.lifetime = ReminderFactLifetime::RunScoped;
    harness.push(scoped);
    assert!(harness.store_has(&snapshot_kind));
    assert!(harness.store_has(&scoped_kind));

    // 新 Run：RunScoped 清空，Snapshot 存活并在新 Run 的时机方法中入队。
    harness.restart_run(vec![snapshot_kind.clone(), scoped_kind.clone()]);
    assert!(
        harness.store_has(&snapshot_kind),
        "Snapshot 事实跨 Run 存活"
    );
    assert!(
        !harness.store_has(&scoped_kind),
        "RunScoped 事实随 Run 创建清空"
    );

    harness.run_started();
    let injection = harness.inject(4096);
    let tail = injection
        .tail_user_message
        .expect("Snapshot 事实跨 Run 后仍注入");
    assert!(tail.contains("kind=\"task-progress\""));
}

#[test]
fn remove_fact_stops_reinjection() {
    let kind = ReminderKind::task_progress();
    let mut harness = Harness::new(vec![kind.clone()]);
    harness.push(task_progress_fact(
        kind.clone(),
        run_start_policy(ReminderPlacement::TailUserMessage),
        2,
        0,
    ));
    harness.run_started();
    assert!(harness.inject(4096).tail_user_message.is_some());

    // 生产者「无内容」→ 显式失效，后续任何时机都不再注入旧值。
    harness.remove(&kind);
    harness.restart_run(vec![kind.clone()]);
    harness.run_started();
    harness.task_mutated();
    assert!(
        harness.inject(4096).tail_user_message.is_none(),
        "remove 后不得重注入陈旧事实"
    );
}

#[test]
fn oneshot_fact_enqueues_immediately_and_reports_uninjected_rollback_key() {
    let kind = ReminderKind::background_process();
    let mut harness = Harness::new(vec![kind.clone()]);

    // OneShot 推送即入队（无需事件触发）。
    harness.push(oneshot_fact(kind.clone(), Some("bgp_1")));
    assert!(!harness.store_has(&kind), "OneShot 不入 store");

    // 预算不足 → 滞留，收口时上报回滚标识供宿主撤销交付标记。
    assert!(harness.inject(1).tail_user_message.is_none());
    assert_eq!(harness.drain_uninjected_oneshot_keys(), vec!["bgp_1"]);

    // 预算充足 → 注入成功，收口不再上报。
    assert!(harness.inject(4096).tail_user_message.is_some());
    assert!(harness.drain_uninjected_oneshot_keys().is_empty());
}

#[test]
fn fact_validity_requires_rollback_key_for_oneshot() {
    let kind = ReminderKind::background_process();
    assert!(oneshot_fact(kind.clone(), Some("bgp_1")).is_valid());
    assert!(!oneshot_fact(kind, None).is_valid(), "OneShot 必须带回滚键");
}
