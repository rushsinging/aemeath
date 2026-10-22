//! 任务注册面契约测试：重复注册拒绝、串行全跑、失败隔离。

use crate::domain::errors::HarnessError;
use crate::domain::task::{CycleOutcome, RsiTask, TaskKind, TaskRegistry};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU32, Ordering};

/// 计数桩任务：记录 run_cycle 次数，可注入失败。
struct CountingTask {
    calls: AtomicU32,
    fail: bool,
}

#[async_trait]
impl RsiTask for CountingTask {
    async fn run_cycle(&mut self) -> Result<CycleOutcome, HarnessError> {
        let calls = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail {
            return Err(HarnessError::invalid(
                crate::constants::ERROR_DOMAIN,
                format!("第 {calls} 次周期失败"),
            ));
        }
        Ok(CycleOutcome {
            summary: format!("ok-{calls}"),
        })
    }
}

/// 重复注册同 kind 必须拒绝（装配错误早暴露）。
#[tokio::test]
async fn duplicate_registration_rejected() {
    let mut registry = TaskRegistry::new();
    registry
        .register(
            TaskKind::HarnessRsi,
            Box::new(CountingTask {
                calls: AtomicU32::new(0),
                fail: false,
            }),
        )
        .expect("首次注册成功");
    let error = registry
        .register(
            TaskKind::HarnessRsi,
            Box::new(CountingTask {
                calls: AtomicU32::new(0),
                fail: false,
            }),
        )
        .expect_err("重复注册必须拒绝");
    assert!(error.message().contains("harness-rsi"));
    assert_eq!(registry.len(), 1);
}

/// 全周期串行执行：每个任务恰好跑一次，按 kind 名稳定排序。
#[tokio::test]
async fn run_all_cycles_runs_each_task_once_in_stable_order() {
    let mut registry = TaskRegistry::new();
    let reflect = CountingTask {
        calls: AtomicU32::new(0),
        fail: false,
    };
    let rsi = CountingTask {
        calls: AtomicU32::new(0),
        fail: false,
    };
    registry
        .register(TaskKind::MemoryReflect, Box::new(reflect))
        .expect("register");
    registry
        .register(TaskKind::HarnessRsi, Box::new(rsi))
        .expect("register");

    let results = registry.run_all_cycles().await;
    assert_eq!(results.len(), 2);
    let (first_kind, first) = &results[0];
    let (second_kind, second) = &results[1];
    // 字典序：harness-rsi < memory-reflect
    assert_eq!(*first_kind, TaskKind::HarnessRsi);
    assert_eq!(*second_kind, TaskKind::MemoryReflect);
    assert!(first.is_ok() && second.is_ok());
}

/// 单任务失败 NEVER 中断其余任务（失败隔离，结果携带 Err）。
#[tokio::test]
async fn failing_task_does_not_block_others() {
    let mut registry = TaskRegistry::new();
    registry
        .register(
            TaskKind::HarnessRsi,
            Box::new(CountingTask {
                calls: AtomicU32::new(0),
                fail: true,
            }),
        )
        .expect("register");
    registry
        .register(
            TaskKind::MemoryReflect,
            Box::new(CountingTask {
                calls: AtomicU32::new(0),
                fail: false,
            }),
        )
        .expect("register");

    let results = registry.run_all_cycles().await;
    assert_eq!(results.len(), 2);
    assert!(results
        .iter()
        .any(|(kind, result)| { *kind == TaskKind::HarnessRsi && result.is_err() }));
    assert!(results
        .iter()
        .any(|(kind, result)| { *kind == TaskKind::MemoryReflect && result.is_ok() }));
}
