use std::sync::{Arc, Mutex};

use super::reminder_pipeline::{ReminderPipeline, ReminderWindowInjection};
use crate::domain::reminder::{
    CompactBehavior, InjectBehavior, RefreshTrigger, ReminderDedup, ReminderEventSource,
    ReminderKind, ReminderPlacement, ReminderPolicy, ReminderPriority, ReminderSnapshot,
    ReminderSource,
};

const LANGUAGE_ZH: &str = "zh";

fn inject_behavior(priority: ReminderPriority) -> InjectBehavior {
    InjectBehavior {
        dedup: ReminderDedup::SkipIfUnchanged,
        priority,
    }
}

/// 测试 source：开闭原则的证据——只实现 `ReminderSource` 即全链路工作。
struct CountingTestSource {
    kind: ReminderKind,
    policy: ReminderPolicy,
    build_count: Mutex<usize>,
    current: Mutex<String>,
}

impl CountingTestSource {
    fn new(kind: ReminderKind, policy: ReminderPolicy) -> Self {
        Self {
            kind,
            policy,
            build_count: Mutex::new(0),
            current: Mutex::new(String::new()),
        }
    }

    fn set_snapshot(&self, data: &str) {
        *self.current.lock().expect("current lock poisoned") = data.to_string();
    }

    fn build_count(&self) -> usize {
        *self.build_count.lock().expect("build_count lock poisoned")
    }
}

impl ReminderSource for CountingTestSource {
    fn kind(&self) -> ReminderKind {
        self.kind.clone()
    }

    fn policy(&self) -> ReminderPolicy {
        self.policy.clone()
    }

    fn build(&self) -> Option<ReminderSnapshot> {
        *self.build_count.lock().expect("build_count lock poisoned") += 1;
        Some(ReminderSnapshot {
            data: self.current.lock().expect("current lock poisoned").clone(),
        })
    }

    fn render(&self, snapshot: &ReminderSnapshot, language: &str) -> String {
        format!("[{language}] {}", snapshot.data)
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
        compact: CompactBehavior::Rebuild,
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

// ---------- 注册 → 触发 → 注入 → 渲染 全链路 ----------

#[test]
fn run_started_source_flows_to_tail_user_message() {
    let source = Arc::new(CountingTestSource::new(
        ReminderKind::task_progress(),
        run_start_policy(ReminderPlacement::TailUserMessage),
    ));
    source.set_snapshot("total=3 completed=1");
    let mut pipeline = ReminderPipeline::new(vec![source.clone()]);

    pipeline.run_started();
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 512);

    let tail = injection
        .tail_user_message
        .expect("Run 启动 reminder 注入尾部 user message");
    assert!(tail.contains("kind=\"task-progress\""), "统一 envelope");
    assert!(tail.contains("version=\"1\""));
    assert!(tail.contains("[zh] total=3 completed=1"), "按语言渲染 body");
    assert_eq!(source.build_count(), 1);
    assert!(injection.system_tail_blocks.is_empty());
}

#[test]
fn event_source_flows_only_on_matching_event() {
    let source = Arc::new(CountingTestSource::new(
        ReminderKind::memory_updated(),
        event_policy("memory"),
    ));
    source.set_snapshot("changed=2");
    let mut pipeline = ReminderPipeline::new(vec![source.clone()]);

    pipeline.handle_event(&ReminderEventSource::new("background_process"));
    assert!(
        pipeline
            .inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 512)
            .tail_user_message
            .is_none(),
        "非匹配事件源不触发"
    );

    pipeline.handle_event(&ReminderEventSource::new("memory"));
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 512);
    assert!(injection
        .tail_user_message
        .expect("匹配事件源触发注入")
        .contains("[zh] changed=2"));
}

#[test]
fn step_interval_rebuilds_snapshot_at_configured_cadence() {
    let source = Arc::new(CountingTestSource::new(
        ReminderKind::task_progress(),
        interval_policy(3, ReminderPlacement::TailUserMessage),
    ));
    let mut pipeline = ReminderPipeline::new(vec![source.clone()]);

    pipeline.step_advanced(1);
    pipeline.step_advanced(2);
    assert!(
        pipeline
            .inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 512)
            .tail_user_message
            .is_none(),
        "间隔未到不重建"
    );

    source.set_snapshot("total=3 completed=2");
    pipeline.step_advanced(3);
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:01+08:00", 512);
    assert!(
        injection
            .tail_user_message
            .expect("间隔到达触发现场重建注入")
            .contains("[zh] total=3 completed=2"),
        "注入的是当下最新快照，NEVER 陈旧 payload"
    );
    assert_eq!(source.build_count(), 1, "只在间隔点 build");
}

#[test]
fn system_tail_source_flows_to_system_blocks() {
    let source = Arc::new(CountingTestSource::new(
        ReminderKind::new("model_guidance_mismatch"),
        run_start_policy(ReminderPlacement::SystemTail),
    ));
    source.set_snapshot("session=a run=b");
    let mut pipeline = ReminderPipeline::new(vec![source]);

    pipeline.run_started();
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 512);

    assert!(injection.tail_user_message.is_none());
    assert_eq!(injection.system_tail_blocks.len(), 1);
    let block = &injection.system_tail_blocks[0];
    assert_eq!(block.kind, "reminder");
    assert!(block.cacheable, "SystemTail 属 cacheable prefix");
    assert!(!block.cache_break);
    assert!(block.content.contains("kind=\"model-guidance-mismatch\""));
}

#[test]
fn compact_committed_reinstates_run_start_snapshot() {
    let source = Arc::new(CountingTestSource::new(
        ReminderKind::new("guidance_sources_changed"),
        run_start_policy(ReminderPlacement::TailUserMessage),
    ));
    source.set_snapshot("guidance changed");
    let mut pipeline = ReminderPipeline::new(vec![source]);

    pipeline.run_started();
    let first = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 512);
    assert!(first.tail_user_message.is_some());

    pipeline.compact_committed();
    let second = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:05+08:00", 512);
    assert!(
        second
            .tail_user_message
            .expect("Reinstate 重新注入")
            .contains("guidance changed"),
        "compact 后 Run 级恒定 reminder 原样复位"
    );
}

#[test]
fn token_budget_defers_low_priority_block_until_next_round() {
    let high = Arc::new(CountingTestSource::new(
        ReminderKind::memory_updated(),
        event_policy("memory"),
    ));
    high.set_snapshot(&"e".repeat(80));
    let low = Arc::new(CountingTestSource::new(
        ReminderKind::task_progress(),
        interval_policy(1, ReminderPlacement::TailUserMessage),
    ));
    low.set_snapshot(&"t".repeat(400));
    let mut pipeline = ReminderPipeline::new(vec![high, low]);

    pipeline.run_started();
    pipeline.handle_event(&ReminderEventSource::new("memory"));
    pipeline.step_advanced(1);

    // 预算只够高优先级块：memory envelope 约 45 tokens，task 约 110 tokens。
    let first = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 60);
    let tail = first.tail_user_message.expect("高优先级块注入");
    assert!(tail.contains("kind=\"memory-updated\""));
    assert!(!tail.contains("kind=\"task-progress\""), "低优先级被截断");

    let second = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:02+08:00", 512);
    assert!(
        second
            .tail_user_message
            .expect("滞留块下一轮补入")
            .contains("kind=\"task-progress\""),
        "截断 entry 滞留回队，NEVER 静默丢弃"
    );
}

#[test]
fn rebuild_flag_rebuilds_from_source_before_injection() {
    let source = Arc::new(CountingTestSource::new(
        ReminderKind::task_progress(),
        interval_policy(1, ReminderPlacement::TailUserMessage),
    ));
    source.set_snapshot("total=3 completed=1");
    let mut pipeline = ReminderPipeline::new(vec![source.clone()]);

    pipeline.step_advanced(1);
    let first = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 512);
    assert!(first.tail_user_message.is_some());

    source.set_snapshot("total=3 completed=3");
    pipeline.compact_committed();
    let second = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:05+08:00", 512);
    assert!(
        second
            .tail_user_message
            .expect("Rebuild 注入前从 source 现场重建")
            .contains("[zh] total=3 completed=3"),
        "Rebuild NEVER 注入 compact 前的陈旧快照"
    );
}

// ---------- 多 reminder 拼装 ----------

#[test]
fn multiple_reminders_merge_into_single_tail_message() {
    let first_source = Arc::new(CountingTestSource::new(
        ReminderKind::memory_updated(),
        event_policy("memory"),
    ));
    first_source.set_snapshot("changed=1");
    let second_source = Arc::new(CountingTestSource::new(
        ReminderKind::task_progress(),
        interval_policy(1, ReminderPlacement::TailUserMessage),
    ));
    second_source.set_snapshot("total=2 completed=0");
    let mut pipeline = ReminderPipeline::new(vec![first_source, second_source]);

    pipeline.run_started();
    pipeline.handle_event(&ReminderEventSource::new("memory"));
    pipeline.step_advanced(1);

    let injection: ReminderWindowInjection =
        pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 512);
    let tail = injection.tail_user_message.expect("两块均有候选");
    assert_eq!(
        tail.matches("<system-reminder").count(),
        2,
        "多 reminder 合并为单条消息多块"
    );
    let memory_pos = tail.find("kind=\"memory-updated\"").expect("memory 块存在");
    let progress_pos = tail.find("kind=\"task-progress\"").expect("task 块存在");
    assert!(
        memory_pos < progress_pos,
        "事件类 priority 高于任务状态类，排在前"
    );
}

// ---------- 注册期 policy 校验：动态 kind 强制 TailUserMessage ----------

struct InvalidPlacementTestSource;

impl ReminderSource for InvalidPlacementTestSource {
    fn kind(&self) -> ReminderKind {
        ReminderKind::memory_updated()
    }

    fn policy(&self) -> ReminderPolicy {
        ReminderPolicy {
            // 违反缓存不变量：事件驱动 kind 不得使用 SystemTail。
            refresh: RefreshTrigger::OnEvent(ReminderEventSource::new("memory")),
            placement: ReminderPlacement::SystemTail,
            inject: InjectBehavior {
                dedup: ReminderDedup::SkipIfUnchanged,
                priority: ReminderPriority::event(),
            },
            compact: CompactBehavior::Drop,
        }
    }

    fn build(&self) -> Option<ReminderSnapshot> {
        Some(ReminderSnapshot {
            data: "changed=1".to_string(),
        })
    }

    fn render(&self, snapshot: &ReminderSnapshot, language: &str) -> String {
        format!("[{language}] {}", snapshot.data)
    }
}

#[test]
fn pipeline_rejects_source_with_invalid_placement_policy() {
    let mut pipeline = ReminderPipeline::new(vec![Arc::new(InvalidPlacementTestSource)]);
    pipeline.run_started();
    pipeline.handle_event(&ReminderEventSource::new("memory"));

    let injection = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:00+08:00", 512);
    assert!(
        injection.tail_user_message.is_none() && injection.system_tail_blocks.is_empty(),
        "违反缓存不变量（动态 refresh + SystemTail）的 source 在注册期被拒绝，零注入"
    );
}

fn user_message_policy() -> ReminderPolicy {
    ReminderPolicy {
        refresh: RefreshTrigger::OnUserMessage,
        placement: ReminderPlacement::TailUserMessage,
        inject: inject_behavior(ReminderPriority::memory_recall()),
        compact: CompactBehavior::Rebuild,
    }
}

#[test]
fn user_message_trigger_rebuilds_on_each_message() {
    let source = Arc::new(CountingTestSource::new(
        ReminderKind::memory_updated(),
        user_message_policy(),
    ));
    source.set_snapshot("recall=第一轮");
    let mut pipeline = ReminderPipeline::new(vec![source.clone()]);

    pipeline.user_message_received();
    let first = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-05T18:00:00+08:00", 512);
    assert!(first
        .tail_user_message
        .expect("用户消息到达应触发注入")
        .contains("[zh] recall=第一轮"));

    source.set_snapshot("recall=第二轮");
    pipeline.user_message_received();
    let second = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-05T18:01:00+08:00", 512);
    assert!(
        second
            .tail_user_message
            .expect("第二条消息应再次触发")
            .contains("[zh] recall=第二轮"),
        "快照替换语义：注入当下最新快照，NEVER 陈旧 payload"
    );
    assert_eq!(source.build_count(), 2, "每条消息现场重建一次");
}

#[test]
fn user_message_trigger_leaves_other_triggers_untouched() {
    let interval_source = Arc::new(CountingTestSource::new(
        ReminderKind::task_progress(),
        interval_policy(3, ReminderPlacement::TailUserMessage),
    ));
    let mut pipeline = ReminderPipeline::new(vec![interval_source.clone()]);

    pipeline.user_message_received();

    assert!(
        pipeline
            .inject_into_window(LANGUAGE_ZH, "2026-10-05T18:00:00+08:00", 512)
            .tail_user_message
            .is_none(),
        "OnUserMessage 不得触发 OnStepInterval source"
    );
    assert_eq!(interval_source.build_count(), 0);
}

#[test]
fn user_message_is_dynamic_and_must_not_use_system_tail() {
    // is_dynamic 不变量：OnUserMessage 内容随消息变化，MUST TailUserMessage。
    let invalid = ReminderPolicy {
        refresh: RefreshTrigger::OnUserMessage,
        placement: ReminderPlacement::SystemTail,
        inject: inject_behavior(ReminderPriority::memory_recall()),
        compact: CompactBehavior::Rebuild,
    };
    assert!(
        !invalid.is_valid(),
        "OnUserMessage + SystemTail 必须被策略校验拒绝"
    );
    assert!(user_message_policy().is_valid());
}

// ---------- #1848 注入清单落盘：pending_persist 生命周期 ----------

#[test]
fn injected_reminders_persist_on_finalize_and_flush_once() {
    // 注入轮：confirm 后记 pending_persist；
    // finalize 提交消息头插入 reminder；flush 幂等（一次后清空）。
    let source = Arc::new(CountingTestSource::new(
        ReminderKind::task_progress(),
        run_start_policy(ReminderPlacement::TailUserMessage),
    ));
    source.set_snapshot("total=3 completed=1");
    let mut pipeline = ReminderPipeline::new(vec![source]);

    pipeline.run_started();
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-05T18:00:00+08:00", 512);
    assert!(injection.tail_user_message.is_some());

    let pending = pipeline.take_pending_persist_messages();
    assert_eq!(pending.len(), 1, "注入轮产出待落盘消息");
    assert!(
        pending[0].text_content().contains("kind=\"task-progress\""),
        "落盘消息带统一 envelope"
    );
    assert_eq!(
        pipeline.take_pending_persist_messages().len(),
        0,
        "take 后清空（幂等）"
    );

    // 第二轮无注入：无新 pending。
    let second = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-05T18:00:01+08:00", 512);
    assert!(second.tail_user_message.is_none());
    assert_eq!(pipeline.take_pending_persist_messages().len(), 0);
}
