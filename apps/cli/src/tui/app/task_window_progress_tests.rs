use super::helpers_tests::make_task_with_ts;
use super::*;

#[test]
fn test_bug32_window_stays_full_with_ttl_pressure() {
    let now: u64 = 10000;
    let mut tasks: Vec<Task> = Vec::new();

    for i in 1..=8 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("old completed {}", i),
            TaskStatus::Completed,
            now - 600,
        ));
    }
    for i in 9..=10 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("recent completed {}", i),
            TaskStatus::Completed,
            now - 10,
        ));
    }
    tasks.push(make_task_with_ts(
        "11",
        "current task",
        TaskStatus::InProgress,
        now,
    ));
    tasks.push(make_task_with_ts(
        "12",
        "pending a",
        TaskStatus::Pending,
        now,
    ));
    tasks.push(make_task_with_ts(
        "13",
        "pending b",
        TaskStatus::Pending,
        now,
    ));

    let result = build_task_window(&tasks, 7, 1);
    let task_lines = result.len() - 1;
    assert_eq!(
        task_lines, 7,
        "expected 7 task lines, got {}: {:?}",
        task_lines, result
    );
    assert!(result.iter().any(|l| l.contains("■ #11 current task")));
    assert!(result.iter().any(|l| l.contains("□ #12")));
    assert!(result.iter().any(|l| l.contains("□ #13")));
    let comp_count = result.iter().filter(|l| l.starts_with('✓')).count();
    assert!(
        comp_count >= 4,
        "expected >= 4 completed shown, got {}",
        comp_count
    );
}

#[test]
fn test_bug32_window_never_shrinks_during_progression() {
    let now: u64 = 10000;
    let max_lines = 7;

    assert_progression_window(make_stage_1(now), max_lines, "stage 1");
    assert_progression_window(make_stage_2(now), max_lines, "stage 2");
    assert_progression_window(make_stage_3(now), max_lines, "stage 3");
    assert_progression_window(make_stage_4(now), max_lines, "stage 4 (no pending)");
}

fn assert_progression_window(tasks: Vec<Task>, max_lines: usize, label: &str) {
    let result = build_task_window(&tasks, max_lines, 1);
    let task_lines = result
        .iter()
        .skip(1)
        .filter(|l| !l.starts_with('…'))
        .count();
    assert_eq!(
        task_lines, max_lines,
        "{}: expected {} task lines, got {}: {:?}",
        label, max_lines, task_lines, result
    );
}

fn make_stage_1(now: u64) -> Vec<Task> {
    let mut tasks = vec![make_task_with_ts("1", "doing", TaskStatus::InProgress, now)];
    for i in 2..=13 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("task {}", i),
            TaskStatus::Pending,
            now,
        ));
    }
    tasks
}

fn make_stage_2(now: u64) -> Vec<Task> {
    let mut tasks = Vec::new();
    for i in 1..=5 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("done {}", i),
            TaskStatus::Completed,
            now - 100 + i as u64,
        ));
    }
    tasks.push(make_task_with_ts("6", "doing", TaskStatus::InProgress, now));
    for i in 7..=13 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("task {}", i),
            TaskStatus::Pending,
            now,
        ));
    }
    tasks
}

fn make_stage_3(now: u64) -> Vec<Task> {
    let mut tasks = Vec::new();
    for i in 1..=10 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("done {}", i),
            TaskStatus::Completed,
            now - 100 + i as u64,
        ));
    }
    tasks.push(make_task_with_ts(
        "11",
        "doing",
        TaskStatus::InProgress,
        now,
    ));
    for i in 12..=13 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("task {}", i),
            TaskStatus::Pending,
            now,
        ));
    }
    tasks
}

fn make_stage_4(now: u64) -> Vec<Task> {
    let mut tasks = Vec::new();
    for i in 1..=12 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("done {}", i),
            TaskStatus::Completed,
            now - 100 + i as u64,
        ));
    }
    tasks.push(make_task_with_ts(
        "13",
        "doing",
        TaskStatus::InProgress,
        now,
    ));
    tasks
}
