use super::*;
use std::{future::pending, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

fn successful_payload() -> CompleteReflectionResult {
    CompleteReflectionResult {
        output: memory::api::reflection::ReflectionOutput::default(),
        input_tokens: 0,
        output_tokens: 0,
        apply_result: None,
        error_category: None,
        record_id: None,
    }
}

fn payload_with_applied_changes(added: usize, outdated_marked: usize) -> CompleteReflectionResult {
    CompleteReflectionResult {
        apply_result: Some(memory::api::reflection::ReflectionApplyResult {
            attempted: added + outdated_marked,
            completed: added + outdated_marked,
            suggestions_added: added,
            outdated_marked,
            superseded: 0,
        }),
        ..successful_payload()
    }
}

fn request() -> ReflectionTaskRequest {
    ReflectionTaskRequest::new(ReflectionTaskTrigger::PreCompact, vec![])
}

fn assert_completed(
    outcome: ReflectionRunOutcome,
    expected_status: ReflectionTaskCompletionStatus,
) -> ReflectionTaskCompletion {
    match outcome {
        ReflectionRunOutcome::Completed(completion) => {
            assert_eq!(completion.status, expected_status);
            completion
        }
        ReflectionRunOutcome::DisabledSkipped => {
            panic!("the run must not be skipped by configuration")
        }
        ReflectionRunOutcome::BusySkipped => {
            panic!("the run must not be skipped as busy")
        }
        ReflectionRunOutcome::Detached => {
            panic!("the run must not be detached in this test")
        }
    }
}

#[tokio::test]
async fn run_awaits_execution_and_reports_succeeded() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(successful_payload())
    });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );
}

#[tokio::test]
async fn each_run_uses_the_executor_currently_installed() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));

    for marker in ["initial", "switched"] {
        let observed = Arc::clone(&observed);
        let marker = marker.to_string();
        assert_completed(
            adapter
                .run_future(
                    ReflectionTaskTrigger::PreCompact,
                    CancellationToken::new(),
                    move |_cancel| async move {
                        observed.lock().await.push(marker);
                        Ok(successful_payload())
                    },
                )
                .await,
            ReflectionTaskCompletionStatus::Succeeded,
        );
    }

    assert_eq!(*observed.lock().await, ["initial", "switched"]);
}

#[tokio::test]
async fn the_run_receives_the_messages_it_was_given() {
    let observed = Arc::new(Mutex::new(Vec::<String>::new()));
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), {
        let observed = Arc::clone(&observed);
        move |request: ReflectionTaskRequest, _cancel| {
            let observed = Arc::clone(&observed);
            async move {
                *observed.lock().await = request
                    .messages
                    .iter()
                    .map(share::message::Message::text_content)
                    .collect();
                Ok(successful_payload())
            }
        }
    });

    assert_completed(
        adapter
            .run(ReflectionTaskRequest::new(
                ReflectionTaskTrigger::PreCompact,
                vec![share::message::Message::user("before compact")],
            ))
            .await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    assert_eq!(*observed.lock().await, ["before compact"]);
}

#[tokio::test]
async fn the_run_cancellation_token_wins_over_the_executor() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        pending::<()>().await;
        #[allow(unreachable_code)]
        Ok(successful_payload())
    });
    let cancel = CancellationToken::new();
    cancel.cancel();

    assert_completed(
        adapter
            .run_future(
                ReflectionTaskTrigger::Interval { run_ordinal: 3 },
                cancel,
                move |_cancel| async {
                    pending::<()>().await;
                    #[allow(unreachable_code)]
                    Ok(successful_payload())
                },
            )
            .await,
        ReflectionTaskCompletionStatus::Cancelled,
    );
}

#[tokio::test]
async fn timeout_reports_timed_out_without_waiting_for_the_executor() {
    let adapter =
        ReflectionTaskAdapter::new(Duration::from_millis(20), |_request, _cancel| async {
            pending::<()>().await;
            #[allow(unreachable_code)]
            Ok(successful_payload())
        });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::TimedOut,
    );
}

#[tokio::test]
async fn applied_changes_accumulate_into_one_notice_taken_exactly_once() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(payload_with_applied_changes(2, 1))
    });

    let completion = assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );
    assert_eq!(
        completion
            .metadata
            .as_ref()
            .map(|item| item.applied_changes()),
        Some(3)
    );
    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    assert_eq!(
        adapter.take_memory_update_notice(),
        Some(MemoryUpdateNotice { changed: 6 })
    );
    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn zero_applied_changes_never_produce_a_notice() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(payload_with_applied_changes(0, 0))
    });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn a_failed_run_never_produces_a_notice() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(CompleteReflectionResult {
            error_category: Some(memory::api::reflection::ReflectionErrorCategory::Apply),
            ..payload_with_applied_changes(3, 0)
        })
    });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Failed,
    );

    assert_eq!(adapter.take_memory_update_notice(), None);
}

#[tokio::test]
async fn a_timed_out_run_never_produces_a_notice() {
    let adapter =
        ReflectionTaskAdapter::new(Duration::from_millis(20), |_request, _cancel| async {
            pending::<()>().await;
            #[allow(unreachable_code)]
            Ok(successful_payload())
        });

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::TimedOut,
    );

    assert_eq!(adapter.take_memory_update_notice(), None);
}

/// Logs use the reflection facts that only a terminal run can report, and the
/// configuration gate reports why it skipped. Both are `info` because each run
/// happens at most once per trigger window.
/// Drain once, then filter: the capture is a single queue, so a per-needle
/// drain would swallow the lines the next needle looks for.
fn captured_info_lines() -> Vec<String> {
    crate::test_log::drain()
        .into_iter()
        .filter(|(level, _)| *level == log::Level::Info)
        .map(|(_, message)| message)
        .collect()
}

fn lines_matching<'a>(lines: &'a [String], needle: &str) -> Vec<&'a String> {
    lines.iter().filter(|line| line.contains(needle)).collect()
}

#[tokio::test(flavor = "current_thread")]
async fn a_terminal_run_logs_its_token_usage_and_applied_counts() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(CompleteReflectionResult {
            input_tokens: 321,
            output_tokens: 123,
            ..payload_with_applied_changes(2, 1)
        })
    });
    let _guard = crate::test_log::begin();

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    let lines = captured_info_lines();
    let tokens = lines_matching(&lines, "[reflection_tokens]");
    assert_eq!(tokens.len(), 1, "{lines:?}");
    assert!(tokens[0].contains("input_tokens=321"), "{}", tokens[0]);
    assert!(tokens[0].contains("output_tokens=123"), "{}", tokens[0]);

    let applied = lines_matching(&lines, "[reflection_applied]");
    assert_eq!(applied.len(), 1, "{lines:?}");
    assert!(applied[0].contains("added=2"), "{}", applied[0]);
    assert!(applied[0].contains("outdated=1"), "{}", applied[0]);

    let terminal = lines_matching(&lines, "[reflection_terminal]");
    assert_eq!(terminal.len(), 1, "{lines:?}");
    assert!(terminal[0].contains("status=succeeded"), "{}", terminal[0]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_run_without_changes_logs_no_applied_line() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(CompleteReflectionResult {
            input_tokens: 7,
            output_tokens: 3,
            ..payload_with_applied_changes(0, 0)
        })
    });
    let _guard = crate::test_log::begin();

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Succeeded,
    );

    let lines = captured_info_lines();
    assert!(
        lines_matching(&lines, "[reflection_applied]").is_empty(),
        "zero changes must stay out of the log: {lines:?}"
    );
    assert_eq!(
        lines_matching(&lines, "[reflection_tokens]").len(),
        1,
        "{lines:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_run_whose_provider_failed_logs_no_applied_line() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Err(ReflectionError::LlmCall)
    });
    let _guard = crate::test_log::begin();

    assert_completed(
        adapter.run(request()).await,
        ReflectionTaskCompletionStatus::Failed,
    );

    let lines = captured_info_lines();
    let terminal = lines_matching(&lines, "[reflection_terminal]");
    assert_eq!(terminal.len(), 1, "{lines:?}");
    assert!(
        terminal[0].contains("error_category=llm"),
        "{}",
        terminal[0]
    );
    assert!(
        lines_matching(&lines, "[reflection_applied]").is_empty(),
        "a run that never applied must not claim a change: {lines:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn each_configuration_gate_reports_its_own_reason() {
    for (config, expected_reason) in [
        (
            share::config::MemoryConfig {
                enabled: false,
                ..share::config::MemoryConfig::default()
            },
            "memory_off",
        ),
        (
            share::config::MemoryConfig {
                enabled: true,
                reflection: share::config::ReflectionConfig {
                    enabled: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            "reflection_off",
        ),
        (
            share::config::MemoryConfig {
                enabled: true,
                reflection: share::config::ReflectionConfig {
                    enabled: true,
                    interval_runs: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
            "interval_zero",
        ),
    ] {
        let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
        let _guard = crate::test_log::begin();
        let binding =
            crate::application::run::run_factory_support::doubles::fake_provider_binding();

        let outcome = adapter
            .run_complete(
                request(),
                config,
                binding.provider.clone(),
                binding.model.clone(),
                binding.max_tokens,
                binding.requested_reasoning,
                "system".to_string(),
                "en".to_string(),
                std::sync::Arc::new(memory::api::NoOpMemory),
                crate::application::reflection::test_support::noop_reflection_history(),
                None,
            )
            .await;

        assert!(matches!(outcome, ReflectionRunOutcome::DisabledSkipped));
        let lines = captured_info_lines();
        let disabled = lines_matching(&lines, "[reflection_disabled]");
        assert_eq!(disabled.len(), 1, "{lines:?}");
        assert!(disabled[0].contains(expected_reason), "{}", disabled[0]);
    }
}

// ---------------------------------------------------------------------------
// 取消的 history 持久化证据（#1797 取消协议：terminal receipt 必须落盘）
// ---------------------------------------------------------------------------

/// 记录 append/upsert 的 history 替身：取消路径的持久化断言需要区分
/// 「写入过 Running」与「终态以 Cancelled 语义 upsert」两个事实。
#[derive(Clone, Default)]
struct RecordingReflectionHistory {
    appends: Arc<Mutex<Vec<memory::api::reflection::ReflectionRecord>>>,
    upserts: Arc<Mutex<Vec<memory::api::reflection::ReflectionRecord>>>,
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryQuery for RecordingReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::reflection::ReflectionSafeSummary>, memory::api::MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryStore for RecordingReflectionHistory {
    async fn append(
        &self,
        record: &memory::api::reflection::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        self.appends.lock().await.push(record.clone());
        Ok(())
    }

    async fn upsert(
        &self,
        record: &memory::api::reflection::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        self.upserts.lock().await.push(record.clone());
        Ok(())
    }
}

// 原取消测试保护 Failed + Cancelled 终态落盘、不遗留 Running、无冲突成功记录。
// 退役后由独立 token 语义测试验证调用方取消不影响异步反思。
// 退役（#2002 D2「cancel 不影响异步」）：预取消 token 不再折叠为
// Completed(Cancelled)——detached 持独立 token 照常执行；替代证据 =
// detached_run_ignores_cancelled_caller_token（预取消 → 终态 Succeeded 落盘）。

// ---------------------------------------------------------------------------
// 反思游标写入链（#1827）：request 携带 coverage_end → Succeeded 落盘写入；
// 取消/失败不推进
// ---------------------------------------------------------------------------

/// Succeeded 且 request 带 coverage_end：终态 upsert 的记录必须携带游标。
#[tokio::test]
async fn succeeded_run_persists_coverage_end_cursor() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(successful_payload())
    });
    let history = RecordingReflectionHistory::default();
    let binding = crate::application::reflection::test_support::static_reflection_binding();
    let mut request = ReflectionTaskRequest::new(
        ReflectionTaskTrigger::Manual,
        vec![share::message::Message::user("delta")],
    );
    request.coverage_end = Some(42);

    let outcome = adapter
        .run_complete(
            request,
            enabled_memory_config(),
            binding.provider.clone(),
            binding.model.clone(),
            binding.max_tokens,
            binding.requested_reasoning,
            "system".to_string(),
            "en".to_string(),
            std::sync::Arc::new(memory::api::NoOpMemory),
            std::sync::Arc::new(history.clone()),
            None,
        )
        .await;

    assert!(
        matches!(outcome, ReflectionRunOutcome::Detached),
        "必须立即 Detached: {outcome:?}"
    );
    let terminal = await_terminal_upsert(&history).await;
    assert_eq!(
        terminal.status,
        memory::api::reflection::ReflectionStatus::Succeeded
    );
    assert_eq!(
        terminal.coverage_end,
        Some(42),
        "Succeeded 落盘必须携带游标"
    );
}

// 退役（#2002 D2）：预取消不再产生 Cancelled 终态，游标随 detached 实际终态推进；
// 替代证据 = succeeded_run_persists_coverage_end_cursor（成功推游标）与
// detached_run_ignores_cancelled_caller_token。

// ---------------------------------------------------------------------------
// 悬挂 running 收口：进程退出留下的超龄 Running 在下次反思执行前
// 被 upsert 为 Failed(Interrupted)；本次反思照常执行。
// ---------------------------------------------------------------------------

/// list 返回一条超龄 Running 的 history 替身；upsert 记录终态写入。
#[derive(Clone, Default)]
struct StaleRunningReflectionHistory {
    upserts: Arc<Mutex<Vec<memory::api::reflection::ReflectionRecord>>>,
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryQuery for StaleRunningReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::reflection::ReflectionSafeSummary>, memory::api::MemoryError> {
        let stale = memory::api::reflection::ReflectionRecord::running(
            "stale-running",
            1,
            memory::api::reflection::ReflectionTrigger::Interval,
        );
        Ok(vec![stale.safe_summary()])
    }
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryStore for StaleRunningReflectionHistory {
    async fn append(
        &self,
        _record: &memory::api::reflection::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        Ok(())
    }

    async fn upsert(
        &self,
        record: &memory::api::reflection::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        self.upserts.lock().await.push(record.clone());
        Ok(())
    }
}

#[tokio::test]
async fn stale_running_records_are_reaped_before_reflection_executes() {
    // 统一异步链路（#2002）：reap 在派发前同步执行；等待 stale 收口记录出现即可，
    // 本次反思终态（detached）与 reap 语义无关。
    let adapter = ReflectionTaskAdapter::production(Duration::from_secs(5));
    let history = StaleRunningReflectionHistory::default();
    let binding = crate::application::run::run_factory_support::doubles::fake_provider_binding();

    let outcome = adapter
        .run_complete(
            ReflectionTaskRequest::new(
                ReflectionTaskTrigger::Manual,
                vec![share::message::Message::user("reflect me")],
            ),
            enabled_memory_config(),
            binding.provider.clone(),
            binding.model.clone(),
            binding.max_tokens,
            binding.requested_reasoning,
            "system".to_string(),
            "en".to_string(),
            std::sync::Arc::new(memory::api::NoOpMemory),
            std::sync::Arc::new(history.clone()),
            None,
        )
        .await;

    assert!(
        matches!(outcome, ReflectionRunOutcome::Detached),
        "必须立即 Detached: {outcome:?}"
    );
    let mut reaped = None;
    for _ in 0..500 {
        let upserts = history.upserts.lock().await;
        reaped = upserts
            .iter()
            .find(|record| record.id == "stale-running")
            .cloned();
        drop(upserts);
        if reaped.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let reaped = reaped.expect("超龄 Running 必须被收口");
    assert_eq!(
        reaped.status,
        memory::api::reflection::ReflectionStatus::Failed
    );
    assert_eq!(
        reaped.error_category,
        Some(memory::api::reflection::ReflectionErrorCategory::Interrupted)
    );
}

// ---------------------------------------------------------------------------
// 统一异步链路（#2002）：三触发一律 detached；BusySkipped 同步判定；
// detached 持独立 token（cancel 不影响异步）
// ---------------------------------------------------------------------------

async fn await_terminal_upsert(
    history: &RecordingReflectionHistory,
) -> memory::api::reflection::ReflectionRecord {
    for _ in 0..500 {
        if let Some(record) = history.upserts.lock().await.last() {
            return record.clone();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("detached 反思未在 5s 内落盘终态记录");
}

fn enabled_memory_config() -> share::config::MemoryConfig {
    share::config::MemoryConfig {
        enabled: true,
        reflection: share::config::ReflectionConfig {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// 三触发点（Interval/PreCompact/Manual）经生产入口 `run_complete` 一律立即
/// 返回 Detached，终态由 detached 任务异步落盘。
#[tokio::test]
async fn all_triggers_now_dispatch_detached_immediately() {
    for trigger in [
        ReflectionTaskTrigger::Interval { run_ordinal: 1 },
        ReflectionTaskTrigger::PreCompact,
        ReflectionTaskTrigger::Manual,
    ] {
        let adapter =
            ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
                Ok(successful_payload())
            });
        let history = RecordingReflectionHistory::default();
        let binding = crate::application::reflection::test_support::static_reflection_binding();

        let outcome = adapter
            .run_complete(
                ReflectionTaskRequest::new(trigger, vec![share::message::Message::user("m")]),
                enabled_memory_config(),
                binding.provider.clone(),
                binding.model.clone(),
                binding.max_tokens,
                binding.requested_reasoning,
                "system".to_string(),
                "en".to_string(),
                std::sync::Arc::new(memory::api::NoOpMemory),
                std::sync::Arc::new(history.clone()),
                None,
            )
            .await;

        assert!(
            matches!(outcome, ReflectionRunOutcome::Detached),
            "{trigger:?} 必须立即返回 Detached"
        );
        let terminal = await_terminal_upsert(&history).await;
        assert_eq!(
            terminal.status,
            memory::api::reflection::ReflectionStatus::Succeeded,
            "{trigger:?} detached 终态必须 Succeeded 落盘"
        );
    }
}

/// 单槽被占（另一反思在跑）：BusySkipped 必须同步返回（engine 才能回填材料）。
/// 用确定性手动占槽验证（run_complete 生产路径走真 provider，无法靠注入
/// executor 挂起占槽）。
#[tokio::test]
async fn run_complete_reports_busy_skipped_synchronously_when_slot_held() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(successful_payload())
    });
    let binding = crate::application::reflection::test_support::static_reflection_binding();

    // 占住单槽：guard 存活期间下一次 run_complete 必须同步 BusySkipped。
    let _slot_guard = adapter
        .hold_slot_for_test()
        .expect("测试开始时单槽必须空闲");

    let outcome = adapter
        .run_complete(
            ReflectionTaskRequest::new(ReflectionTaskTrigger::PreCompact, vec![]),
            enabled_memory_config(),
            binding.provider.clone(),
            binding.model.clone(),
            binding.max_tokens,
            binding.requested_reasoning,
            "system".to_string(),
            "en".to_string(),
            std::sync::Arc::new(memory::api::NoOpMemory),
            crate::application::reflection::test_support::noop_reflection_history(),
            None,
        )
        .await;

    assert!(
        matches!(outcome, ReflectionRunOutcome::BusySkipped),
        "单槽被占必须同步 BusySkipped: {outcome:?}"
    );
}

/// detached 持独立 token：调用方 cancel（含 Manual Esc / step 收口）不影响异步
/// 反思，终态仍 Succeeded（非 Cancelled）。
#[tokio::test]
async fn detached_run_ignores_cancelled_caller_token() {
    let adapter = ReflectionTaskAdapter::new(Duration::from_secs(5), |_request, _cancel| async {
        Ok(successful_payload())
    });
    let history = RecordingReflectionHistory::default();
    let binding = crate::application::reflection::test_support::static_reflection_binding();
    let cancel = CancellationToken::new();
    cancel.cancel();

    let outcome = adapter
        .run_complete(
            ReflectionTaskRequest::new(ReflectionTaskTrigger::Manual, vec![]),
            enabled_memory_config(),
            binding.provider.clone(),
            binding.model.clone(),
            binding.max_tokens,
            binding.requested_reasoning,
            "system".to_string(),
            "en".to_string(),
            std::sync::Arc::new(memory::api::NoOpMemory),
            std::sync::Arc::new(history.clone()),
            None,
        )
        .await;

    assert!(matches!(outcome, ReflectionRunOutcome::Detached));
    let terminal = await_terminal_upsert(&history).await;
    assert_eq!(
        terminal.status,
        memory::api::reflection::ReflectionStatus::Succeeded,
        "调用方 token 已 cancel 不得影响 detached 执行"
    );
}
