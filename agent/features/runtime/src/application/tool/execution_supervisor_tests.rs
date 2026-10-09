use super::earliest_deadline;
use std::time::{Duration, SystemTime};

#[test]
fn supervisor_uses_earliest_deadline() {
    let now = SystemTime::now();
    assert_eq!(
        earliest_deadline(
            Some(now + Duration::from_secs(30)),
            Some(now + Duration::from_secs(20)),
            Some(now + Duration::from_secs(10)),
        ),
        Some(now + Duration::from_secs(10))
    );
}

use crate::application::context::coordination::ContextCoordinator;
use crate::application::tool::execution_supervisor::{
    dispatch_started_time, SupervisedToolCall, ToolExecutionSupervisor,
};
use async_trait::async_trait;
use context::SessionId;
use context::{
    CompactOutcome, CompactRequestData, CompactionDecisionData, ContextAppendData,
    ContextPortError, ContextRequestData, ContextWindowData, ManualCompactRequestData,
    ToolCallIdentityData, ToolReceiptMutationData, ToolReceiptMutationReceiptData,
};
use sdk::{RunId, RunStepId};
use share::ids::BackgroundProcessId;
use std::sync::{Arc, Mutex};
use tools::published::execution::{
    CancellationSignal, ToolExecutionContext, ToolExecutionOutcome, ToolExecutionPort,
    ToolInvocation,
};
use tools::test_support::{sequential_test_tool_catalog, TestToolExecutionContextBuilder};

// ── fakes ────────────────────────────────────────────────────────────

/// 可控时长的假执行端口：sleep 指定时长后返回成功。
struct SleepingToolPort {
    sleep: Duration,
}

#[async_trait]
impl ToolExecutionPort for SleepingToolPort {
    async fn execute(
        &self,
        _invocation: ToolInvocation,
        _context: &ToolExecutionContext,
    ) -> ToolExecutionOutcome {
        tokio::time::sleep(self.sleep).await;
        ToolExecutionOutcome::success_text("sleep tool finished")
    }
}

/// 永不取消的假信号（前台 cancel 分支不在本组测试范围）。
struct NeverCancelSignal;

#[async_trait]
impl CancellationSignal for NeverCancelSignal {
    fn is_cancelled(&self) -> bool {
        false
    }
    async fn cancelled(&self) {
        std::future::pending::<()>().await
    }
    fn child_signal(&self) -> Arc<dyn CancellationSignal> {
        Arc::new(Self)
    }
}

/// 记录 receipt 状态推进序列的假 ContextPort。
struct RecordingContextPort {
    states: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl context::ContextPort for RecordingContextPort {
    async fn build_window(
        &self,
        _request: &ContextRequestData,
    ) -> Result<ContextWindowData, ContextPortError> {
        Err(ContextPortError::SessionRepository(
            "test port 不支持 build_window".into(),
        ))
    }
    async fn needs_compaction(
        &self,
        _request: &ContextRequestData,
    ) -> Result<CompactionDecisionData, ContextPortError> {
        Err(ContextPortError::SessionRepository(
            "test port 不支持 needs_compaction".into(),
        ))
    }
    async fn compact(
        &self,
        _request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        Err(ContextPortError::SessionRepository(
            "test port 不支持 compact".into(),
        ))
    }
    async fn manual_compact(
        &self,
        _request: &ManualCompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        Err(ContextPortError::SessionRepository(
            "test port 不支持 manual_compact".into(),
        ))
    }
    async fn clear_session(
        &self,
        _session_id: &context::SessionId,
    ) -> Result<(), ContextPortError> {
        Ok(())
    }
    async fn advance_tool_receipt(
        &self,
        mutation: ToolReceiptMutationData,
    ) -> Result<ToolReceiptMutationReceiptData, context::ToolReceiptMutationError> {
        self.states
            .lock()
            .expect("receipt 状态记录锁")
            .push(format!("{:?}", mutation.next));
        Ok(ToolReceiptMutationReceiptData {
            receipt: context::ToolCallReceiptData {
                identity: mutation.identity,
                input_preview: mutation.input_preview.unwrap_or_default(),
                state: mutation.next,
            },
            changed: true,
        })
    }
    async fn append_and_persist(
        &self,
        _append: &ContextAppendData,
    ) -> Result<context::AppendReceiptData, context::ContextAppendError> {
        Err(context::ContextAppendError::Storage(
            "test port 不支持 append_and_persist".to_string(),
        ))
    }
}

// ── helpers ──────────────────────────────────────────────────────────

fn supervised_call(
    context: ToolExecutionContext,
    background_threshold: Option<Duration>,
    run_deadline: Option<SystemTime>,
) -> SupervisedToolCall {
    SupervisedToolCall {
        identity: ToolCallIdentityData {
            session_id: SessionId::new("session-1"),
            run_id: RunId::new("run-1"),
            step_id: RunStepId::new("step-1"),
            runtime_call_id: "runtime-call-1".to_string(),
            provider_call_id: None,
            tool_name: "SleepTool".to_string(),
            call_index: 0,
            agent: false,
        },
        invocation: ToolInvocation::new(
            "SleepTool",
            serde_json::json!({}),
            context.scope().clone(),
        ),
        context,
        input_preview: "{}".to_string(),
        run_deadline,
        cancellation: Arc::new(NeverCancelSignal),
        child_cancellation: tokio_util::sync::CancellationToken::new(),
        background_threshold,
    }
}

fn supervisor_with(sleep: Duration) -> (ToolExecutionSupervisor, Arc<Mutex<Vec<String>>>) {
    let states = Arc::new(Mutex::new(Vec::new()));
    let port = RecordingContextPort {
        states: Arc::clone(&states),
    };
    let supervisor = ToolExecutionSupervisor::new(
        Arc::new(SleepingToolPort { sleep }),
        sequential_test_tool_catalog("SleepTool", 300),
        ContextCoordinator::new(Arc::new(port)),
    );
    (supervisor, states)
}

fn test_context() -> ToolExecutionContext {
    TestToolExecutionContextBuilder::new(std::env::temp_dir()).build()
}

async fn wait_for_terminal_state(states: &Arc<Mutex<Vec<String>>>, timeout: Duration) -> String {
    let started = std::time::Instant::now();
    loop {
        let terminal = states
            .lock()
            .expect("receipt 状态记录锁")
            .iter()
            .rev()
            .find(|state| state.starts_with("Terminal("))
            .cloned();
        if let Some(state) = terminal {
            return state;
        }
        assert!(
            started.elapsed() < timeout,
            "后台驱动未在 {timeout:?} 内写入终态"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// 从 Success content 提取纯文本（测试断言用）。
fn success_text(outcome: &ToolExecutionOutcome) -> String {
    let ToolExecutionOutcome::Success(success) = outcome else {
        return String::new();
    };
    success
        .content
        .iter()
        .map(|block| block.text.clone())
        .collect::<Vec<_>>()
        .join("")
}

// ── tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn execute_within_threshold_completes_on_fast_path() {
    let (supervisor, states) = supervisor_with(Duration::from_millis(20));

    let (outcome, _duration) = supervisor
        .execute(supervised_call(
            test_context(),
            Some(Duration::from_secs(5)),
            None,
        ))
        .await
        .expect("快路径执行应成功");

    assert!(
        success_text(&outcome).contains("sleep tool finished"),
        "阈值内完成应返回真实结果：{}",
        success_text(&outcome)
    );
    let recorded = states.lock().expect("receipt 状态记录锁").clone();
    assert!(recorded.iter().any(|state| state == "Running"));
    assert!(recorded
        .last()
        .is_some_and(|state| state.starts_with("Terminal(")));
}

#[tokio::test]
async fn execute_exceeding_threshold_returns_placeholder_and_backgrounds_receipt() {
    let (supervisor, states) = supervisor_with(Duration::from_millis(400));

    let started = std::time::Instant::now();
    let (outcome, _) = supervisor
        .execute(supervised_call(
            test_context(),
            Some(Duration::from_millis(40)),
            None,
        ))
        .await
        .expect("转后台执行应成功");
    let foreground_wait = started.elapsed();

    assert!(
        foreground_wait < Duration::from_millis(300),
        "转后台后前台等待应立即结束，实际 {foreground_wait:?}"
    );
    let placeholder_text = success_text(&outcome);
    assert!(
        placeholder_text.contains("background"),
        "占位文案应说明已转后台：{placeholder_text}"
    );
    assert!(
        placeholder_text.split_whitespace().count() <= 12,
        "占位文案应精简（process id + running in background）：{placeholder_text}"
    );
    assert!(
        placeholder_text
            .split_whitespace()
            .find(|word| word.contains("bgp_"))
            .map(|word| { word.trim_start_matches('(').trim_end_matches(['.', ')']) })
            .and_then(|process_id| BackgroundProcessId::parse(process_id).ok())
            .is_some(),
        "占位文案应携带合法 process id：{placeholder_text}"
    );
    assert!(
        states
            .lock()
            .expect("receipt 状态记录锁")
            .iter()
            .any(|state| state == "Backgrounded"),
        "转后台应推进 receipt 至 Backgrounded"
    );
}

#[tokio::test]
async fn backgrounded_receipt_reaches_terminal_after_real_completion() {
    let (supervisor, states) = supervisor_with(Duration::from_millis(120));

    let (outcome, _) = supervisor
        .execute(supervised_call(
            test_context(),
            Some(Duration::from_millis(30)),
            None,
        ))
        .await
        .expect("转后台执行应成功");
    assert!(matches!(outcome, ToolExecutionOutcome::Success(_)));

    let terminal = wait_for_terminal_state(&states, Duration::from_secs(3)).await;
    assert!(
        terminal.contains("Success"),
        "后台真实完成应写入 Success 终态：{terminal}"
    );
}

#[tokio::test]
async fn execute_without_threshold_keeps_current_behavior() {
    let (supervisor, states) = supervisor_with(Duration::from_millis(20));

    let (outcome, _) = supervisor
        .execute(supervised_call(test_context(), None, None))
        .await
        .expect("无阈值配置应保持现状行为");

    assert!(matches!(outcome, ToolExecutionOutcome::Success(_)));
    let recorded = states.lock().expect("receipt 状态记录锁").clone();
    assert!(
        !recorded.iter().any(|state| state == "Backgrounded"),
        "无阈值不得转后台"
    );
    assert!(recorded
        .last()
        .is_some_and(|state| state.starts_with("Terminal(")));
}

#[tokio::test]
async fn hard_deadline_takes_precedence_over_threshold() {
    let (supervisor, _states) = supervisor_with(Duration::from_millis(500));

    let (outcome, _) = supervisor
        .execute(supervised_call(
            test_context(),
            Some(Duration::from_secs(10)),
            Some(SystemTime::now() + Duration::from_millis(50)),
        ))
        .await
        .expect("deadline 分支应处理");

    assert!(
        matches!(outcome, ToolExecutionOutcome::CancellationUnconfirmed(_)),
        "硬 deadline 应先于阈值触发；NonCooperative 工具超时按 #1440 语义收敛为 CancellationUnconfirmed：{outcome:?}"
    );
}

// 占位结果构造单测：Success 且文案非空。
#[tokio::test]
async fn placeholder_outcome_is_error_free_success() {
    let (supervisor, _) = supervisor_with(Duration::from_millis(400));
    let (outcome, _) = supervisor
        .execute(supervised_call(
            test_context(),
            Some(Duration::from_millis(20)),
            None,
        ))
        .await
        .expect("转后台执行应成功");
    assert!(!success_text(&outcome).is_empty(), "占位文案不得为空");
}

// ── #252 PR2：spawn body 终态通知路由（L4 场景） ─────────────────────

#[tokio::test]
async fn background_terminal_sends_wakeup_signal_without_active_run() {
    let (mut supervisor, states) = supervisor_with(Duration::from_millis(50));
    let background = std::sync::Arc::new(
        crate::application::background_process::session_runtime::BackgroundProcessRuntime::new(),
    );
    // 无 active run（不 bind registry）→ 终态路由 wakeup 信号。
    supervisor = supervisor.with_background_runtime(Some(background.clone()));

    let (outcome, _) = supervisor
        .execute(supervised_call(
            test_context(),
            Some(Duration::from_millis(10)),
            None,
        ))
        .await
        .expect("execute 不可失败");
    assert!(
        matches!(outcome, ToolExecutionOutcome::Success(_)),
        "占位结果"
    );
    let _ = states;

    // spawn body 真实完成（sleep 50ms）后：监督器登记 + 终态 + wakeup 信号。
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let terminal_count = background
            .supervisor()
            .snapshots()
            .iter()
            .filter(|record| record.is_terminal())
            .count();
        if terminal_count == 1 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "后台进程应在超时前到达终态（监督器账本）"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // 终态未通知条目仍在（无 active run 走信号，不 build take），
    // 等 wakeup run 启动后 handle_event 消费。
    let waiter = background.take_wakeup_waiter().expect("等待端未被取走");
    let mut waiter = waiter;
    // waiter 可探测信号（unbounded，已投递）。
    assert!(
        waiter.try_wait().is_some(),
        "无 active Run 时终态必须发 wakeup 信号"
    );
}

#[tokio::test]
async fn background_terminal_task_recorded_with_terminal_kind() {
    let (mut supervisor, _) = supervisor_with(Duration::from_millis(30));
    let background = std::sync::Arc::new(
        crate::application::background_process::session_runtime::BackgroundProcessRuntime::new(),
    );
    supervisor = supervisor.with_background_runtime(Some(background.clone()));

    let (outcome, _) = supervisor
        .execute(supervised_call(
            test_context(),
            Some(Duration::from_millis(10)),
            None,
        ))
        .await
        .expect("execute 不可失败");
    assert!(matches!(outcome, ToolExecutionOutcome::Success(_)));

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let snapshots = background.supervisor().snapshots();
        if snapshots.iter().any(|record| record.is_terminal()) {
            let record = snapshots
                .iter()
                .find(|record| record.is_terminal())
                .expect("已确认存在终态");
            assert_eq!(record.identity.tool_name, "SleepTool");
            assert!(
                matches!(
                    record.terminal_kind(),
                    Some(crate::domain::background_process::BackgroundProcessTerminalKind::Success)
                ),
                "SleepTool 正常完成应记 Success 终态"
            );
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "后台进程应在超时前到达终态"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// ── 派发时刻换算（Instant → SystemTime） ─────────────────────────────

#[test]
fn dispatch_started_time_is_elapsed_before_now() {
    let started = std::time::Instant::now() - std::time::Duration::from_secs(3);
    let wall = dispatch_started_time(started);
    let elapsed = std::time::SystemTime::now()
        .duration_since(wall)
        .expect("换算时刻不晚于当前墙钟");
    assert!(
        elapsed.as_millis() >= 3_000,
        "换算结果应早于当前约 3s，实际 {elapsed:?}"
    );
    assert!(
        elapsed.as_millis() < 5_000,
        "换算误差不应膨胀，实际 {elapsed:?}"
    );
}
