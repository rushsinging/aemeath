//! 后台任务 session 运行时（#252 PR2）：监督器 + 唤醒信箱的 session 级装配。

use std::sync::{Arc, Mutex};

use super::supervisor::BackgroundTaskSupervisor;
use crate::application::session::wakeup::{self, WakeupNotifier, WakeupWaiter};

/// Session 级后台任务运行时：挂在 `SessionRuntime`（跨 Run 共享）。
///
/// - `supervisor`：任务事实账本（登记 / 状态推进 / 未通知终态 take）。
/// - 唤醒信箱：任务终态且无 active Run 时发信号，session driver idle
///   等待点 select（Wakeup Run，D13）。
pub(crate) struct BackgroundTaskRuntime {
    supervisor: Arc<BackgroundTaskSupervisor>,
    notifier: WakeupNotifier,
    waiter: Mutex<Option<WakeupWaiter>>,
    active_run:
        std::sync::OnceLock<Arc<crate::application::run::active_registry::ActiveRunRegistry>>,
    /// #252 PR3：账本持久化目标（blob + session id；session 就绪后绑定）。
    persistence: std::sync::OnceLock<(std::sync::Arc<dyn storage::AtomicBlobPort>, String)>,
    /// #252 PR3：事件 sink 工厂（spinner 活动数事件；per-chat 创建）。
    event_sink_factory: std::sync::OnceLock<
        std::sync::Arc<
            dyn Fn(
                    tokio::sync::mpsc::UnboundedSender<sdk::ChatEvent>,
                ) -> crate::application::loop_engine::chat::ChatEventSinkHandle
                + Send
                + Sync,
        >,
    >,
}

/// 终态通知路由决策（纯判定，#252 PR2 §4.2/§4.4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BackgroundNotifyRoute {
    /// 有 active Main Run：`background_task` reminder 事件注入该 Run。
    Reminder(sdk::RunId),
    /// 无 active Run：wakeup 信号驱动 Wakeup Run。
    WakeupSignal,
}

impl BackgroundTaskRuntime {
    pub(crate) fn new() -> Self {
        let (notifier, waiter) = wakeup::wakeup_channel();
        Self {
            supervisor: Arc::new(BackgroundTaskSupervisor::new()),
            notifier,
            waiter: Mutex::new(Some(waiter)),
            active_run: std::sync::OnceLock::new(),
            persistence: std::sync::OnceLock::new(),
            event_sink_factory: std::sync::OnceLock::new(),
        }
    }

    /// 绑定事件 sink 工厂（session 就绪后；spinner 活动数事件）。
    pub(crate) fn bind_event_sink_factory(
        &self,
        factory: std::sync::Arc<
            dyn Fn(
                    tokio::sync::mpsc::UnboundedSender<sdk::ChatEvent>,
                ) -> crate::application::loop_engine::chat::ChatEventSinkHandle
                + Send
                + Sync,
        >,
    ) {
        let _ = self.event_sink_factory.set(factory);
    }

    /// 发送后台任务活动数事件（try_send：sink 满时丢弃，显示尽力而为）。
    pub(crate) fn emit_active_count(&self) {
        let Some(factory) = self.event_sink_factory.get() else {
            return;
        };
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let sink = factory(sender);
        let active = self
            .supervisor()
            .snapshots()
            .iter()
            .filter(|record| !record.is_terminal())
            .count();
        use crate::application::loop_engine::chat::ChatEventSink as _;
        sink.try_send_event(
            crate::application::loop_engine::chat::RuntimeStreamEvent::BackgroundTaskCountChanged {
                active,
            },
        );
        // receiver 由 sink 生命周期外丢弃（try_send 即投递到 tx）。
        drop(receiver);
    }

    /// 绑定 active run registry（SessionRuntime::new 收尾时调用，一次绑定）。
    pub(crate) fn bind_active_run(
        &self,
        registry: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
    ) {
        let _ = self.active_run.set(registry);
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
    ) -> Self {
        let runtime = Self::new();
        runtime.bind_active_run(active_run);
        runtime
    }

    /// 通知路由判定：有 active Main Run → reminder 事件；无 → wakeup 信号。
    pub(crate) fn notify_route(&self) -> BackgroundNotifyRoute {
        let Some(registry) = self.active_run.get() else {
            return BackgroundNotifyRoute::WakeupSignal;
        };
        match registry.current_main_run_id() {
            Some(run_id) => BackgroundNotifyRoute::Reminder(run_id),
            None => BackgroundNotifyRoute::WakeupSignal,
        }
    }

    /// 任务终态通知（spawn body 调用）：推进监督器账本、持久化快照并按路由送达。
    ///
    /// 重复终态幂等（finish 返回 changed=false 时不再通知）。
    pub(crate) async fn notify_terminal(
        &self,
        context: &crate::application::context::coordination::ContextCoordinator,
        task_id: &share::ids::BackgroundTaskId,
        kind: crate::domain::background_task::BackgroundTaskTerminalKind,
        terminal_output: Option<String>,
    ) {
        let finished = self
            .supervisor
            .finish(task_id, kind, terminal_output)
            .unwrap_or(false);
        if !finished {
            return;
        }
        // #252 PR3：终态快照落盘（绑定失败/未绑定时静默跳过——
        // 持久化尽力而为，不影响通知路由）。
        if let Err(error) = self.persist_snapshot().await {
            log::warn!(
                target: crate::LOG_TARGET,
                "background task ledger persist failed: {error}"
            );
        }
        // #252 PR3：活动数 -1 → spinner 显示。
        self.emit_active_count();
        match self.notify_route() {
            BackgroundNotifyRoute::Reminder(run_id) => {
                context.reminder_handle_event(
                    &run_id,
                    &context::ReminderEventSource::background_task(),
                );
            }
            BackgroundNotifyRoute::WakeupSignal => {
                if let Err(error) = self.notifier.wakeup() {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "background wakeup signal lost (session exiting): {error:?}"
                    );
                }
            }
        }
    }

    pub(crate) fn supervisor(&self) -> Arc<BackgroundTaskSupervisor> {
        self.supervisor.clone()
    }

    /// 取走等待端（session driver 首次接线；一个 session 一个等待端）。
    pub(crate) fn take_wakeup_waiter(&self) -> Option<WakeupWaiter> {
        self.waiter.lock().expect("后台任务唤醒信箱锁中毒").take()
    }
}

#[cfg(test)]
#[path = "session_runtime_tests.rs"]
mod tests;

// ── #252 PR3：BackgroundTaskAccess 端口实现（BackgroundTasks tool 数据源） ──

impl tools::BackgroundTaskAccess for BackgroundTaskRuntime {
    fn list_tasks(&self) -> Vec<tools::types::background_tasks::BackgroundTaskSummaryData> {
        self.supervisor()
            .snapshots()
            .into_iter()
            .map(|record| task_summary_data(&record))
            .collect()
    }

    fn task_status(
        &self,
        task_id: &str,
    ) -> Option<tools::types::background_tasks::BackgroundTaskDetailData> {
        let parsed = share::ids::BackgroundTaskId::parse_display(task_id).ok()?;
        let record = self.supervisor().snapshot(&parsed)?;
        let deadline_remaining_ms = match &record.state {
            crate::domain::background_task::BackgroundTaskState::Backgrounded {
                deadline_snapshot: Some(deadline),
                ..
            } => deadline
                .duration_since(std::time::SystemTime::now())
                .ok()
                .map(|remaining| remaining.as_millis() as u64),
            _ => None,
        };
        Some(tools::types::background_tasks::BackgroundTaskDetailData {
            summary: task_summary_data(&record),
            deadline_remaining_ms,
            total_written_bytes: 0,
        })
    }

    fn read_task_log(
        &self,
        task_id: &str,
        cursor: Option<u64>,
        max_bytes: usize,
    ) -> Option<tools::types::background_tasks::BackgroundTaskLogData> {
        let parsed = share::ids::BackgroundTaskId::parse_display(task_id).ok()?;
        let (text, cursor, total_written) = self
            .supervisor()
            .read_task_log(&parsed, cursor, max_bytes)?;
        Some(tools::types::background_tasks::BackgroundTaskLogData {
            text,
            cursor,
            total_written,
        })
    }

    fn stop_task(
        &self,
        task_id: &str,
    ) -> Result<tools::types::background_tasks::BackgroundTaskStopData, String> {
        let parsed = share::ids::BackgroundTaskId::parse_display(task_id)
            .map_err(|error| format!("invalid task id {task_id}: {error}"))?;
        let outcome = self
            .supervisor()
            .stop_task(&parsed)
            .map_err(|error| error.to_string())?;
        let data = match outcome {
            super::supervisor::StopRequestOutcome::SignalSent { state } => {
                tools::types::background_tasks::BackgroundTaskStopData {
                    signal_sent: true,
                    state: state_vocabulary(&state).to_string(),
                }
            }
            super::supervisor::StopRequestOutcome::AlreadyTerminal(kind) => {
                tools::types::background_tasks::BackgroundTaskStopData {
                    signal_sent: false,
                    state: terminal_vocabulary(&kind).to_string(),
                }
            }
        };
        Ok(data)
    }
}

/// 记录 → 查询摘要（状态词汇 + 运行时长）。
fn task_summary_data(
    record: &crate::domain::background_task::BackgroundTaskRecord,
) -> tools::types::background_tasks::BackgroundTaskSummaryData {
    let state = match record.terminal_kind() {
        Some(kind) => terminal_vocabulary(&kind).to_string(),
        None => state_vocabulary(&record.state).to_string(),
    };
    let duration_ms = std::time::SystemTime::now()
        .duration_since(record.created_at)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    tools::types::background_tasks::BackgroundTaskSummaryData {
        task_id: record.task_id.display(),
        tool_name: record.identity.tool_name.clone(),
        state,
        summary: record.invocation_summary.clone(),
        duration_ms: Some(duration_ms),
    }
}

fn state_vocabulary(state: &crate::domain::background_task::BackgroundTaskState) -> &'static str {
    match state {
        crate::domain::background_task::BackgroundTaskState::ForegroundWaiting => {
            "foreground_waiting"
        }
        crate::domain::background_task::BackgroundTaskState::Backgrounded { .. } => "backgrounded",
        crate::domain::background_task::BackgroundTaskState::Terminal(_) => "terminal",
    }
}

fn terminal_vocabulary(
    kind: &crate::domain::background_task::BackgroundTaskTerminalKind,
) -> &'static str {
    match kind {
        crate::domain::background_task::BackgroundTaskTerminalKind::Success => "succeeded",
        crate::domain::background_task::BackgroundTaskTerminalKind::Failure => "failed",
        crate::domain::background_task::BackgroundTaskTerminalKind::TimedOut => "timed_out",
        crate::domain::background_task::BackgroundTaskTerminalKind::Stopped => "stopped",
        crate::domain::background_task::BackgroundTaskTerminalKind::Invalidated { .. } => {
            "invalidated"
        }
    }
}

// ── #252 PR3：账本持久化（snapshot 落盘 / resume 恢复） ───────────────

impl BackgroundTaskRuntime {
    /// 绑定持久化目标（session 就绪后调用一次）。
    pub(crate) async fn bind_persistence(
        &self,
        blob: std::sync::Arc<dyn storage::AtomicBlobPort>,
        session_id: String,
    ) -> Result<(), storage::StorageError> {
        // 提前构造 key 验证 session id 合法（fail fast）。
        ledger_key(&session_id)?;
        let _ = self.persistence.set((blob, session_id));
        Ok(())
    }

    /// 全量快照落盘（终态推进后调用；任务量小，原子写可接受）。
    pub(crate) async fn persist_snapshot(&self) -> Result<(), storage::StorageError> {
        let Some((blob, session_id)) = self.persistence.get() else {
            return Ok(());
        };
        let key = ledger_key(session_id)?;
        let bytes = serde_json::to_vec(&self.supervisor().snapshots()).map_err(|error| {
            storage::StorageError::new(storage::StorageErrorKind::Io, error.to_string())
        })?;
        blob.write_atomic(
            &key,
            &bytes,
            storage::WriteOptionsData::new(storage::DurabilityData::ProcessCrashSafe),
        )
        .await?;
        Ok(())
    }

    /// 从快照恢复（resume）：终态记录原样恢复，非终态标
    /// `Invalidated(ProcessExit)`（执行体随原进程消亡）。返回恢复条数。
    pub(crate) async fn restore_from_snapshot(
        &self,
        blob: &std::sync::Arc<dyn storage::AtomicBlobPort>,
        session_id: &str,
    ) -> Result<usize, storage::StorageError> {
        let key = ledger_key(session_id)?;
        let bytes = match blob.read(&key, storage::GenerationData::Primary).await? {
            storage::ReadOutcomeData::Found(entry) => entry.bytes().to_vec(),
            storage::ReadOutcomeData::NotFound => return Ok(0),
        };
        let records: Vec<crate::domain::background_task::BackgroundTaskRecord> =
            serde_json::from_slice(&bytes).map_err(|error| {
                storage::StorageError::new(storage::StorageErrorKind::Io, error.to_string())
            })?;
        let restored = self.supervisor().restore_records(records);
        Ok(restored)
    }
}

/// 账本 key：`background-task/<session-id>`。
fn ledger_key(session_id: &str) -> Result<storage::StorageKeyData, storage::StorageError> {
    let segment: storage::SafePathSegmentData = session_id
        .parse()
        .map_err(|error: storage::StorageError| error)?;
    storage::StorageKeyData::new(storage::StorageNamespaceData::BackgroundTask, vec![segment])
}
