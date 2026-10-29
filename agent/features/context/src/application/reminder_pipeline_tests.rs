//! Reminder 管线行为测试（#2146 数据化接口：push_fact + 时机方法）。

use super::reminder_pipeline::ReminderPipeline;
use crate::domain::reminder::{
    CompactBehavior, InjectBehavior, RefreshTrigger, ReminderDedup, ReminderEnqueue,
    ReminderEventSource, ReminderFactData, ReminderKind, ReminderPlacement, ReminderPolicy,
    ReminderPriority,
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
        data: serde_json::to_string(
            &crate::domain::InvocationReminderData::GuidanceSourcesChanged {
                paths: vec![text.to_string()],
            },
        )
        .expect("载荷序列化"),
    }
}

fn pipeline_with(allowed: Vec<ReminderKind>) -> ReminderPipeline {
    ReminderPipeline::new(allowed)
}

#[test]
fn run_started_fact_flows_to_tail_user_message() {
    let kind = ReminderKind::task_progress();
    let mut pipeline = pipeline_with(vec![kind.clone()]);
    pipeline.push_fact(task_progress_fact(
        kind,
        run_start_policy(ReminderPlacement::TailUserMessage),
        3,
        1,
    ));

    pipeline.run_started();
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, AT, 512);

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
    let mut pipeline = pipeline_with(vec![kind.clone()]);
    pipeline.push_fact(text_fact(kind, event_policy("memory"), "changed=2"));

    pipeline.handle_event(&ReminderEventSource::new("background_process"));
    assert!(
        pipeline
            .inject_into_window(LANGUAGE_ZH, AT, 512)
            .tail_user_message
            .is_none(),
        "非匹配事件源不触发"
    );

    pipeline.handle_event(&ReminderEventSource::new("memory"));
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, AT, 512);
    assert!(injection.tail_user_message.is_some(), "匹配事件源触发注入");
}

#[test]
fn step_interval_enqueues_cached_fact_at_configured_cadence() {
    let kind = ReminderKind::task_progress();
    let mut pipeline = pipeline_with(vec![kind.clone()]);
    pipeline.push_fact(task_progress_fact(
        kind.clone(),
        interval_policy(3, ReminderPlacement::TailUserMessage),
        3,
        1,
    ));

    pipeline.step_advanced(1);
    pipeline.step_advanced(2);
    assert!(
        pipeline
            .inject_into_window(LANGUAGE_ZH, AT, 512)
            .tail_user_message
            .is_none(),
        "间隔未到不入队"
    );

    pipeline.push_fact(task_progress_fact(
        kind,
        interval_policy(3, ReminderPlacement::TailUserMessage),
        3,
        2,
    ));
    pipeline.step_advanced(3);
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:01+08:00", 512);
    assert!(
        injection.tail_user_message.is_some(),
        "间隔到达触发注入（内容为最新推送快照）"
    );
}

#[test]
fn system_tail_fact_flows_to_system_blocks() {
    let kind = ReminderKind::new("model_guidance_mismatch");
    let mut pipeline = pipeline_with(vec![kind.clone()]);
    pipeline.push_fact(text_fact(
        kind,
        run_start_policy(ReminderPlacement::SystemTail),
        "mismatch",
    ));

    pipeline.run_started();
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, AT, 512);

    assert_eq!(injection.system_tail_blocks.len(), 1);
    assert!(injection.tail_user_message.is_none());
    assert!(injection.system_tail_blocks[0]
        .content
        .contains("system-reminder"));
}

#[test]
fn compact_committed_reinstates_run_start_fact() {
    let kind = ReminderKind::task_progress();
    let mut pipeline = pipeline_with(vec![kind.clone()]);
    pipeline.push_fact(task_progress_fact(
        kind,
        run_start_policy(ReminderPlacement::TailUserMessage),
        3,
        1,
    ));
    pipeline.run_started();
    let _ = pipeline.inject_into_window(LANGUAGE_ZH, AT, 512);

    pipeline.compact_committed();
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:01:00+08:00", 512);
    assert!(
        injection.tail_user_message.is_some(),
        "Reinstate 型 fact 在 compact 后重注入"
    );
}

#[test]
fn token_budget_defers_block_until_next_round() {
    let low = ReminderKind::new("low_priority");
    let high = ReminderKind::new("high_priority");
    let mut pipeline = pipeline_with(vec![low.clone(), high.clone()]);
    pipeline.push_fact(text_fact(
        high.clone(),
        run_start_policy(ReminderPlacement::TailUserMessage),
        "high",
    ));
    pipeline.push_fact(text_fact(
        low.clone(),
        ReminderPolicy {
            inject: inject_behavior(ReminderPriority::environment()),
            ..run_start_policy(ReminderPlacement::TailUserMessage)
        },
        "low",
    ));
    pipeline.run_started();

    // 极小预算：只容纳一条。
    let first = pipeline.inject_into_window(LANGUAGE_ZH, AT, 8);
    let second = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:00:10+08:00", 4096);
    assert!(first.tail_user_message.is_some() || second.tail_user_message.is_some());
}

#[test]
fn deferred_fact_rebuilds_from_cache_on_next_round() {
    let kind = ReminderKind::task_progress();
    let mut pipeline = pipeline_with(vec![kind.clone()]);
    pipeline.push_fact(task_progress_fact(
        kind,
        interval_policy(1, ReminderPlacement::TailUserMessage),
        1,
        0,
    ));
    pipeline.run_started();
    pipeline.step_advanced(1);

    // 极小预算：本条 defer（滞留），队列置 rebuild 标记。
    let deferred = pipeline.inject_into_window(LANGUAGE_ZH, AT, 1);
    assert!(deferred.tail_user_message.is_none(), "预算不足滞留");

    // 下一轮：rebuild 标记从缓存 fact 重建并入队。
    let retried = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:02:00+08:00", 4096);
    assert!(
        retried.tail_user_message.is_some(),
        "滞留事实下一轮由缓存重建注入"
    );
}

#[test]
fn multiple_facts_merge_into_single_tail_message() {
    let a = ReminderKind::new("kind_a");
    let b = ReminderKind::new("kind_b");
    let mut pipeline = pipeline_with(vec![a.clone(), b.clone()]);
    pipeline.push_fact(text_fact(
        a,
        run_start_policy(ReminderPlacement::TailUserMessage),
        "a",
    ));
    pipeline.push_fact(text_fact(
        b,
        run_start_policy(ReminderPlacement::TailUserMessage),
        "b",
    ));
    pipeline.run_started();

    let tail = pipeline
        .inject_into_window(LANGUAGE_ZH, AT, 4096)
        .tail_user_message
        .expect("多条 fact 合并为单条尾部消息");
    assert!(tail.contains("kind=\"kind-a\""));
    assert!(tail.contains("kind=\"kind-b\""));
}

#[test]
fn pipeline_rejects_fact_with_invalid_placement_policy() {
    // 动态 refresh（OnStepInterval）强制 TailUserMessage：SystemTail 组合拒收。
    let kind = ReminderKind::task_progress();
    let mut pipeline = pipeline_with(vec![kind.clone()]);
    pipeline.push_fact(task_progress_fact(
        kind,
        interval_policy(3, ReminderPlacement::SystemTail),
        3,
        1,
    ));
    pipeline.run_started();
    pipeline.step_advanced(3);

    let injection = pipeline.inject_into_window(LANGUAGE_ZH, AT, 512);
    assert!(
        injection.tail_user_message.is_none() && injection.system_tail_blocks.is_empty(),
        "非法 placement 组合拒收（不入队）"
    );
}

#[test]
fn pipeline_rejects_fact_with_unregistered_kind() {
    let kind = ReminderKind::task_progress();
    let mut pipeline = pipeline_with(Vec::new());
    pipeline.push_fact(task_progress_fact(
        kind,
        run_start_policy(ReminderPlacement::TailUserMessage),
        3,
        1,
    ));
    pipeline.run_started();
    assert!(
        pipeline
            .inject_into_window(LANGUAGE_ZH, AT, 512)
            .tail_user_message
            .is_none(),
        "非白名单 kind 拒收"
    );
}

#[test]
fn user_message_fact_rebuilds_on_each_message() {
    let kind = ReminderKind::memory_recall();
    let mut pipeline = pipeline_with(vec![kind.clone()]);
    pipeline.push_fact(text_fact(kind.clone(), user_message_policy(), "first"));
    pipeline.user_message_received();
    let first = pipeline.inject_into_window(LANGUAGE_ZH, AT, 4096);
    assert!(first.tail_user_message.is_some());

    pipeline.push_fact(text_fact(kind, user_message_policy(), "second"));
    pipeline.user_message_received();
    let second = pipeline.inject_into_window(LANGUAGE_ZH, "2026-10-04T01:03:00+08:00", 4096);
    assert!(second.tail_user_message.is_some(), "每轮用户消息重建注入");
}

#[test]
fn user_message_trigger_leaves_other_triggers_untouched() {
    let run_kind = ReminderKind::task_progress();
    let msg_kind = ReminderKind::memory_recall();
    let mut pipeline = pipeline_with(vec![run_kind.clone(), msg_kind.clone()]);
    pipeline.push_fact(task_progress_fact(
        run_kind,
        run_start_policy(ReminderPlacement::TailUserMessage),
        1,
        0,
    ));
    pipeline.push_fact(text_fact(msg_kind, user_message_policy(), "recall"));

    // 只触发 user_message，不触发 run_started：RunStart 型不应入队。
    pipeline.user_message_received();
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, AT, 4096);
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
    let mut pipeline = pipeline_with(vec![kind.clone()]);
    let mut policy = user_message_policy();
    policy.placement = ReminderPlacement::SystemTail;
    pipeline.push_fact(text_fact(kind, policy, "recall"));
    pipeline.user_message_received();
    let injection = pipeline.inject_into_window(LANGUAGE_ZH, AT, 512);
    assert!(
        injection.tail_user_message.is_none() && injection.system_tail_blocks.is_empty(),
        "动态 refresh 配 SystemTail 必须拒收"
    );
}
