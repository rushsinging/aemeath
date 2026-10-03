use crate::domain::reminder::*;

fn snapshot_of(data: &str) -> ReminderSnapshot {
    ReminderSnapshot {
        data: data.to_string(),
    }
}

fn static_policy(refresh: RefreshTrigger, placement: ReminderPlacement) -> ReminderPolicy {
    ReminderPolicy {
        refresh,
        placement,
        inject: InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::environment(),
        },
        compact: CompactBehavior::Reinstate,
    }
}

fn queue_with_entries() -> ReminderQueue {
    let mut queue = ReminderQueue::new();
    queue.push_snapshot(
        ReminderKind::task_progress(),
        snapshot_of("total=3 completed=1"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::task_state(),
        },
    );
    queue.push_event(
        ReminderKind::memory_updated(),
        snapshot_of("changed=2"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::event(),
        },
    );
    queue
}

fn seqs_of(candidates: &[InjectionCandidate]) -> Vec<u64> {
    candidates.iter().map(|c| c.seq).collect()
}

fn kinds_of(candidates: &[InjectionCandidate]) -> Vec<String> {
    candidates
        .iter()
        .map(|c| c.kind.as_str().to_string())
        .collect()
}

// ---------- 入队：快照替换与事件累积 ----------

#[test]
fn push_snapshot_replaces_pending_entry_of_same_kind() {
    let mut queue = ReminderQueue::new();
    queue.push_snapshot(
        ReminderKind::task_progress(),
        snapshot_of("total=3 completed=1"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::task_state(),
        },
    );
    queue.push_snapshot(
        ReminderKind::task_progress(),
        snapshot_of("total=3 completed=2"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::task_state(),
        },
    );

    let candidates = queue.begin_injection();
    let progress: Vec<_> = candidates
        .iter()
        .filter(|c| c.kind == ReminderKind::task_progress())
        .collect();
    assert_eq!(progress.len(), 1, "同 kind 快照只保留最新 entry");
    assert_eq!(progress[0].snapshot.data, "total=3 completed=2");
}

#[test]
fn push_event_accumulates_entries_of_same_kind() {
    let mut queue = ReminderQueue::new();
    queue.push_event(
        ReminderKind::memory_updated(),
        snapshot_of("changed=1"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::event(),
        },
    );
    queue.push_event(
        ReminderKind::memory_updated(),
        snapshot_of("changed=3"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::event(),
        },
    );

    let candidates = queue.begin_injection();
    let memory: Vec<_> = candidates
        .iter()
        .filter(|c| c.kind == ReminderKind::memory_updated())
        .collect();
    assert_eq!(memory.len(), 2, "事件类同 kind entry 各自累积");
    assert_eq!(memory[0].snapshot.data, "changed=1");
    assert_eq!(memory[1].snapshot.data, "changed=3");
}

// ---------- 注入：排序与 dedup ----------

#[test]
fn begin_injection_orders_entries_by_priority_descending() {
    let mut queue = queue_with_entries();

    let candidates = queue.begin_injection();
    assert_eq!(
        kinds_of(&candidates),
        vec!["memory_updated".to_string(), "task_progress".to_string(),],
        "事件类优先于任务状态类"
    );
}

#[test]
fn confirm_injection_skips_same_fingerprint_next_round() {
    let mut queue = ReminderQueue::new();
    queue.push_snapshot(
        ReminderKind::task_progress(),
        snapshot_of("total=3 completed=1"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::task_state(),
        },
    );

    let mut queue = queue_with_entries();
    let candidates = queue.begin_injection();
    queue.confirm_injection(&seqs_of(&candidates));
    assert!(queue.begin_injection().is_empty(), "队列为空");

    // 内容未变化：同 fingerprint 重新入队也不注入
    queue.push_snapshot(
        ReminderKind::task_progress(),
        snapshot_of("total=3 completed=1"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::task_state(),
        },
    );
    assert!(
        queue.begin_injection().is_empty(),
        "SkipIfUnchanged 对相同 fingerprint 去重"
    );

    // 内容变化：重新注入
    queue.push_snapshot(
        ReminderKind::task_progress(),
        snapshot_of("total=3 completed=2"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::task_state(),
        },
    );
    let candidates = queue.begin_injection();
    assert_eq!(kinds_of(&candidates), vec!["task_progress".to_string()]);
}

// ---------- 预算截断与滞留 ----------

#[test]
fn defer_injection_keeps_entry_for_next_round_with_priority() {
    let mut queue = queue_with_entries();

    let candidates = queue.begin_injection();
    let deferred_seq = candidates
        .iter()
        .find(|c| c.kind == ReminderKind::task_progress())
        .map(|c| c.seq)
        .expect("task_progress 在候选中");
    let injected_seqs: Vec<u64> = candidates
        .iter()
        .map(|c| c.seq)
        .filter(|seq| *seq != deferred_seq)
        .collect();
    queue.confirm_injection(&injected_seqs);
    queue.defer_injection(&[deferred_seq]);

    let next_round = queue.begin_injection();
    assert_eq!(
        kinds_of(&next_round),
        vec!["task_progress".to_string()],
        "滞留 entry 下一轮仍在候选中，NEVER 静默丢弃"
    );
    queue.confirm_injection(&seqs_of(&next_round));
}

// ---------- compact 处置 ----------

#[test]
fn apply_compact_outcome_rebuild_clears_entries_and_flags_rebuild() {
    let mut queue = queue_with_entries();

    queue.apply_compact_outcome(&[(ReminderKind::task_progress(), CompactBehavior::Rebuild)]);

    let candidates = queue.begin_injection();
    assert!(
        !kinds_of(&candidates).contains(&"task_progress".to_string()),
        "Rebuild 清空旧 entry"
    );
    assert!(
        queue.needs_rebuild(&ReminderKind::task_progress()),
        "Rebuild 标记注入前强制重建"
    );
    assert!(!queue.needs_rebuild(&ReminderKind::memory_updated()));
}

#[test]
fn apply_compact_outcome_reinstate_requeues_last_snapshot() {
    let mut queue = queue_with_entries();
    let candidates = queue.begin_injection();
    queue.confirm_injection(&seqs_of(&candidates));

    queue.apply_compact_outcome(&[(ReminderKind::task_progress(), CompactBehavior::Reinstate)]);

    let reinstated = queue.begin_injection();
    assert_eq!(
        kinds_of(&reinstated),
        vec!["task_progress".to_string()],
        "Reinstate 将最近快照重新入队"
    );
    let progress = reinstated
        .iter()
        .find(|c| c.kind == ReminderKind::task_progress())
        .expect("task_progress 在候选中");
    assert_eq!(progress.snapshot.data, "total=3 completed=1");
}

#[test]
fn apply_compact_outcome_drop_removes_and_blocks_kind() {
    let mut queue = queue_with_entries();

    queue.apply_compact_outcome(&[(ReminderKind::memory_updated(), CompactBehavior::Drop)]);

    let candidates = queue.begin_injection();
    assert!(!kinds_of(&candidates).contains(&"memory_updated".to_string()));
    queue.push_event(
        ReminderKind::memory_updated(),
        snapshot_of("changed=9"),
        InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::event(),
        },
    );
    assert!(
        !kinds_of(&queue.begin_injection()).contains(&"memory_updated".to_string()),
        "Drop 后同 kind 再入队被封锁"
    );
}

// ---------- policy 校验：动态 kind 强制 TailUserMessage ----------

#[test]
fn policy_validation_rejects_dynamic_refresh_with_system_tail() {
    let static_ok = static_policy(RefreshTrigger::OnRunStart, ReminderPlacement::SystemTail);
    assert!(static_ok.is_valid(), "Run 级恒定内容允许 SystemTail");

    let interval_bad = static_policy(
        RefreshTrigger::OnStepInterval(4),
        ReminderPlacement::SystemTail,
    );
    assert!(
        !interval_bad.is_valid(),
        "OnStepInterval MUST NOT 使用 SystemTail（缓存不变量）"
    );

    let event_bad = static_policy(
        RefreshTrigger::OnEvent(ReminderEventSource::new("memory")),
        ReminderPlacement::SystemTail,
    );
    assert!(
        !event_bad.is_valid(),
        "OnEvent MUST NOT 使用 SystemTail（缓存不变量）"
    );

    let event_ok = static_policy(
        RefreshTrigger::OnEvent(ReminderEventSource::new("memory")),
        ReminderPlacement::TailUserMessage,
    );
    assert!(event_ok.is_valid());
}

// ---------- envelope 组装 ----------

#[test]
fn compose_reminder_blocks_renders_unified_envelope() {
    let block = compose_reminder_envelope(&ReminderEnvelopeInput {
        kind: "task_progress".to_string(),
        body: "任务进度：1/3".to_string(),
        at: "2026-10-04T01:00:00+08:00".to_string(),
        seq: 7,
    });
    assert_eq!(
        block,
        "<system-reminder kind=\"task-progress\" version=\"1\" at=\"2026-10-04T01:00:00+08:00\" seq=\"7\">\n任务进度：1/3\n</system-reminder>"
    );
}

#[test]
fn compose_reminder_user_message_merges_blocks_into_single_message() {
    let first = compose_reminder_envelope(&ReminderEnvelopeInput {
        kind: "memory_updated".to_string(),
        body: "记忆更新：2 条".to_string(),
        at: "2026-10-04T01:00:00+08:00".to_string(),
        seq: 3,
    });
    let second = compose_reminder_envelope(&ReminderEnvelopeInput {
        kind: "task_progress".to_string(),
        body: "任务进度：1/3".to_string(),
        at: "2026-10-04T01:00:01+08:00".to_string(),
        seq: 7,
    });

    let message = compose_tail_user_message(&[first.clone(), second.clone()]);
    assert_eq!(message, format!("{first}\n{second}"));
}
