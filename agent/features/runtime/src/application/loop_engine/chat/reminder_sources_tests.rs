use super::*;
use crate::application::constants::TASK_PROGRESS_REFRESH_INTERVAL_STEPS;
use crate::application::loop_engine::chat::reminder_sources::{
    RunStartFactReminderSource, TaskProgressReminderSource,
};

fn access_with_progress(completed: usize) -> task::TaskStore {
    let store = task::TaskStore::new();
    let access: &dyn task::TaskAccess = &store;
    access
        .create_batch(
            task::BatchCreateSpecData::try_new("batch".into()).unwrap(),
            1,
        )
        .unwrap();
    let mut task_ids = Vec::new();
    for index in 0..2 {
        let task_spec = task::TaskCreateSpecData::try_new(
            format!("任务 {index}"),
            String::new(),
            None,
            task::TaskPriorityData::Normal,
        )
        .unwrap();
        task_ids.push(access.create_task(task_spec, 2).unwrap().value.id());
    }
    for task_id in task_ids.iter().take(completed).cloned() {
        access
            .transition_with_progress(
                task_id,
                task::TaskStatusData::Completed,
                (completed + 10) as u64,
            )
            .unwrap();
    }
    store
}

#[test]
fn task_progress_source_builds_snapshot_from_task_access_and_renders_via_context() {
    let source = TaskProgressReminderSource::new(
        Arc::new(access_with_progress(1)),
        share::config::TaskListConfig::default().max_lines,
    );

    assert_eq!(source.kind().as_str(), "task_progress");
    let snapshot = source.build().expect("有 active batch 任务");
    let decoded: context::InvocationReminderData =
        serde_json::from_str(&snapshot.data).expect("快照为 InvocationReminderData JSON");
    match decoded {
        context::InvocationReminderData::TaskProgress(progress) => {
            assert_eq!(progress.total, 2);
            assert_eq!(progress.completed, 1);
        }
        other => panic!("期望 TaskProgress，得到 {other:?}"),
    }

    let rendered = source.render(&snapshot, "zh");
    assert!(
        rendered.contains("当前任务进度："),
        "渲染委托 context 文案单一真相"
    );
}

#[test]
fn task_progress_source_builds_none_without_active_batch() {
    let empty_store = task::TaskStore::new();
    let source = TaskProgressReminderSource::new(
        Arc::new(empty_store),
        share::config::TaskListConfig::default().max_lines,
    );
    assert!(source.build().is_none(), "无任务周期 source 本轮不入队");
}

#[test]
fn task_progress_source_policy_declares_interval_rebuild_and_tail_placement() {
    let source = TaskProgressReminderSource::new(
        Arc::new(access_with_progress(0)),
        share::config::TaskListConfig::default().max_lines,
    );
    let policy = source.policy();
    assert!(
        matches!(
            policy.refresh,
            context::RefreshTrigger::OnStepInterval(interval)
                if interval == TASK_PROGRESS_REFRESH_INTERVAL_STEPS
        ),
        "周期重注入（run_started 的 step=0 提供首次注入）"
    );
    assert_eq!(
        policy.placement,
        context::ReminderPlacement::TailUserMessage
    );
    assert!(matches!(policy.compact, context::CompactBehavior::Rebuild));
}

#[test]
fn run_start_fact_source_carries_frozen_data_and_matching_policy() {
    let guidance = RunStartFactReminderSource::guidance_sources_changed();
    assert_eq!(guidance.kind().as_str(), "guidance_sources_changed");
    let snapshot = guidance.build().expect("事实型恒有内容");
    assert_eq!(
        serde_json::from_str::<context::InvocationReminderData>(&snapshot.data).unwrap(),
        context::InvocationReminderData::GuidanceSourcesChanged
    );
    let policy = guidance.policy();
    assert_eq!(policy.placement, context::ReminderPlacement::SystemTail);

    let mismatch =
        RunStartFactReminderSource::model_guidance_mismatch("session-model", "run-model");
    assert_eq!(mismatch.kind().as_str(), "model_guidance_mismatch");
    assert_eq!(
        mismatch.policy().placement,
        context::ReminderPlacement::SystemTail
    );
    let rendered = mismatch.render(&mismatch.build().unwrap(), "zh");
    assert!(rendered.contains("session-model"));
    assert!(rendered.contains("run-model"));

    let memory = RunStartFactReminderSource::memory_updated(3);
    assert_eq!(memory.kind().as_str(), "memory_updated");
    assert_eq!(
        memory.policy().placement,
        context::ReminderPlacement::TailUserMessage
    );
    assert!(matches!(
        memory.policy().compact,
        context::CompactBehavior::Drop
    ));
}
