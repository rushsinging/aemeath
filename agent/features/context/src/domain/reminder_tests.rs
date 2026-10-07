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

// ---------- run_started 以 step=0 推进：interval source Run 启动即触发 ----------

struct StaticDataTestSource {
    kind: ReminderKind,
    policy: ReminderPolicy,
    snapshot: String,
}

impl ReminderSource for StaticDataTestSource {
    fn kind(&self) -> ReminderKind {
        self.kind.clone()
    }

    fn policy(&self) -> ReminderPolicy {
        self.policy.clone()
    }

    fn build(&self) -> Option<ReminderSnapshot> {
        Some(ReminderSnapshot {
            data: self.snapshot.clone(),
        })
    }

    fn render(&self, snapshot: &ReminderSnapshot, language: &str) -> String {
        format!("[{language}] {}", snapshot.data)
    }
}

#[test]
fn run_started_triggers_step_interval_source_at_step_zero() {
    let source = std::sync::Arc::new(StaticDataTestSource {
        kind: ReminderKind::task_progress(),
        policy: interval_policy_for_domain(4),
        snapshot: "total=3 completed=0".to_string(),
    });
    let mut pipeline = crate::application::reminder_pipeline::ReminderPipeline::new(vec![source]);

    pipeline.run_started();
    let injection = pipeline.inject_into_window("zh", "2026-10-04T01:00:00+08:00", 512);
    assert!(
        injection
            .tail_user_message
            .expect("interval source 在 Run 启动（step=0）触发")
            .contains("[zh] total=3 completed=0"),
        "0 是任意间隔的倍数：TaskProgress 无需 OnRunStart + OnStepInterval 双声明"
    );
}

fn interval_policy_for_domain(interval: u32) -> ReminderPolicy {
    ReminderPolicy {
        refresh: RefreshTrigger::OnStepInterval(interval),
        placement: ReminderPlacement::TailUserMessage,
        inject: InjectBehavior {
            dedup: ReminderDedup::SkipIfUnchanged,
            priority: ReminderPriority::task_state(),
        },
        compact: CompactBehavior::Rebuild,
    }
}

// ---------- build 返回 None：本轮无内容不入队 ----------

struct OptionalTestSource {
    kind: ReminderKind,
    snapshot: Option<String>,
}

impl ReminderSource for OptionalTestSource {
    fn kind(&self) -> ReminderKind {
        self.kind.clone()
    }

    fn policy(&self) -> ReminderPolicy {
        static_policy(
            RefreshTrigger::OnRunStart,
            ReminderPlacement::TailUserMessage,
        )
    }

    fn build(&self) -> Option<ReminderSnapshot> {
        self.snapshot
            .as_ref()
            .map(|data| ReminderSnapshot { data: data.clone() })
    }

    fn render(&self, snapshot: &ReminderSnapshot, language: &str) -> String {
        format!("[{language}] {}", snapshot.data)
    }
}

#[test]
fn build_returning_none_does_not_enqueue() {
    let source = std::sync::Arc::new(OptionalTestSource {
        kind: ReminderKind::task_progress(),
        snapshot: None,
    });
    let mut pipeline = crate::application::reminder_pipeline::ReminderPipeline::new(vec![source]);

    pipeline.run_started();
    assert!(
        pipeline
            .inject_into_window("zh", "2026-10-04T01:00:00+08:00", 512)
            .tail_user_message
            .is_none(),
        "无内容（如当前无任务）的周期 source 本轮不注入"
    );
}

// ---------- InvocationReminderData serde 载体与 body 渲染 ----------

#[test]
fn invocation_reminder_data_serde_round_trip_drives_fingerprint_stability() {
    let data = crate::domain::InvocationReminderData::TaskProgress(
        crate::domain::TaskProgressReminderData {
            total: 3,
            completed: 1,
            items: vec![crate::domain::TaskProgressReminderItemData {
                sequence: 7,
                subject: "实现队列".to_string(),
                status: crate::domain::TaskProgressStatus::InProgress,
                blocked_by_sequences: vec![2, 4],
            }],
            hidden_count: 1,
        },
    );
    let encoded = serde_json::to_string(&data).expect("序列化");
    let decoded: crate::domain::InvocationReminderData =
        serde_json::from_str(&encoded).expect("反序列化");
    assert_eq!(decoded, data, "round-trip 稳定（fingerprint 依赖）");

    let again = serde_json::to_string(&decoded).expect("再序列化");
    assert_eq!(encoded, again, "同一数据编码确定");
}

#[test]
fn render_invocation_reminder_body_covers_all_kinds_bilingually() {
    let progress = crate::domain::InvocationReminderData::TaskProgress(
        crate::domain::TaskProgressReminderData {
            total: 2,
            completed: 1,
            items: vec![crate::domain::TaskProgressReminderItemData {
                sequence: 1,
                subject: "任务 A&B".to_string(),
                status: crate::domain::TaskProgressStatus::Completed,
                blocked_by_sequences: vec![],
            }],
            hidden_count: 0,
        },
    );
    let zh = render_invocation_reminder_body(&progress, "zh");
    assert!(zh.contains("当前任务进度："));
    assert!(zh.contains("任务 A&amp;B"), "subject 经 HTML 转义");
    let en = render_invocation_reminder_body(&progress, "en");
    assert!(en.contains("Current task progress:"));

    let guidance = crate::domain::InvocationReminderData::GuidanceSourcesChanged {
        paths: vec!["~/.agents/guidance/_default.md".to_string()],
    };
    let guidance_zh = render_invocation_reminder_body(&guidance, "zh");
    assert!(guidance_zh.contains("guidance 来源已变更"));
    assert!(
        guidance_zh.contains("用 Read 工具重新读取"),
        "Remind 形态带 Read 引导（specs/3.9 §155）：{guidance_zh}"
    );
    assert!(guidance_zh.contains("~/.agents/guidance/_default.md"));
    let guidance_en = render_invocation_reminder_body(&guidance, "en");
    assert!(guidance_en.contains("Use the Read tool"));

    let guidance_no_paths =
        crate::domain::InvocationReminderData::GuidanceSourcesChanged { paths: vec![] };
    assert!(
        render_invocation_reminder_body(&guidance_no_paths, "zh").contains("guidance 来源已变更")
    );

    let mismatch = crate::domain::InvocationReminderData::ModelGuidanceMismatch {
        session_model_id: "a<b".to_string(),
        run_model_id: "m".to_string(),
    };
    assert!(render_invocation_reminder_body(&mismatch, "zh").contains("a&lt;b"));

    let memory = crate::domain::InvocationReminderData::MemoryUpdated { changed: 5 };
    assert!(render_invocation_reminder_body(&memory, "zh").contains("记忆已更新 5 条"));
}

#[test]
fn background_task_completed_kind_and_render_are_bilingual() {
    let data = crate::domain::InvocationReminderData::background_task_completed(vec![
        crate::domain::BackgroundTaskReminderItemData {
            task_id: "task-01a2b3c4".to_string(),
            tool_name: "Bash".to_string(),
            status: crate::domain::BackgroundTaskCompletionStatus::Succeeded,
            output_tail: "test result: ok. 3 passed".to_string(),
        },
        crate::domain::BackgroundTaskReminderItemData {
            task_id: "task-05e6f7a8".to_string(),
            tool_name: "Agent".to_string(),
            status: crate::domain::BackgroundTaskCompletionStatus::Failed,
            output_tail: String::new(),
        },
    ]);
    assert_eq!(data.kind(), "background_task");

    let zh = render_invocation_reminder_body(&data, "zh");
    assert!(zh.contains("后台任务已完成"), "zh 标题：{zh}");
    assert!(zh.contains("task-01a2b3c4"));
    assert!(zh.contains("Bash"));
    assert!(zh.contains("成功"));
    assert!(zh.contains("失败"));
    assert!(zh.contains("test result: ok. 3 passed"));
    assert!(
        zh.contains("日志或后续输出可用 background_tasks 工具查询"),
        "引导查询：{zh}"
    );

    let en = render_invocation_reminder_body(&data, "en");
    assert!(en.contains("Background task completed"), "en 标题：{en}");
    assert!(en.contains("succeeded"));
    assert!(en.contains("failed"));
    assert!(en.contains("Use the background_tasks tool"));
}

#[test]
fn background_task_item_fields_are_serializable_for_fingerprint() {
    // SkipIfUnchanged fingerprint 对 data serde 全量计算，字段必须可序列化。
    let data = crate::domain::InvocationReminderData::background_task_completed(vec![
        crate::domain::BackgroundTaskReminderItemData {
            task_id: "task-1".to_string(),
            tool_name: "Bash".to_string(),
            status: crate::domain::BackgroundTaskCompletionStatus::TimedOut,
            output_tail: "x".to_string(),
        },
    ]);
    let json = serde_json::to_string(&data).expect("reminder data 可序列化");
    assert!(json.contains("\"task-1\""));
    assert!(json.contains("timed_out"));
    let round_trip: crate::domain::InvocationReminderData =
        serde_json::from_str(&json).expect("可反序列化");
    assert_eq!(round_trip, data);
}
