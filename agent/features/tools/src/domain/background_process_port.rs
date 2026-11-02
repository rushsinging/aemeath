//! 后台进程查询端口（#252）：tools 侧契约，runtime 提供实现。
//! Watch 等待（#1931）由本模块自由函数 `wait_task` 编排：基于非阻塞
//! `task_status` 轮询 + timeout + cancellation，NEVER 在调用方盲 sleep。

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::constants::{WATCH_POLL_INTERVAL_MS, WATCH_STREAM_LOG_CHUNK_BYTES};
use super::context::{CancellationSignal, ProgressSink};
use super::tool_types::ToolProgressEvent;
use super::types::background_processes::{
    is_background_process_terminal_state, BackgroundProcessDetailData, BackgroundProcessLogData,
    BackgroundProcessStopData, BackgroundProcessSummaryData, BackgroundProcessWatchOutcome,
    BackgroundProcessWatchResult,
};

/// 后台进程账本读取与停止请求端口。
///
/// 实现方持有 session 级监督器；查询 / stop 方法只读或幂等信号，
/// NEVER 在这些方法内阻塞等待任务真实终态。等待语义见 [`wait_task`]。
pub trait BackgroundProcessAccess: Send + Sync {
    /// 活动与近期任务摘要（按创建序）。
    fn list_tasks(&self) -> Vec<BackgroundProcessSummaryData>;

    /// 单任务详情；未知 id 返回 None。
    fn task_status(&self, task_id: &str) -> Option<BackgroundProcessDetailData>;

    /// 日志读取（增量游标；非消耗性）；未知 id 返回 None。
    fn read_task_log(
        &self,
        task_id: &str,
        cursor: Option<u64>,
        max_bytes: usize,
    ) -> Option<BackgroundProcessLogData>;

    /// stop 请求；未知 id 返回 Err（用户可读消息）。
    fn stop_task(&self, task_id: &str) -> Result<BackgroundProcessStopData, String>;
}

/// 可换绑端口源（session 创建后绑定实现，先例 MemoryPortSource）。
pub trait BackgroundProcessAccessSource: Send + Sync {
    fn current(&self) -> Arc<dyn BackgroundProcessAccess>;
}

/// 等待任务终态、超时或 cancellation。
///
/// - 未知 id → `Err`
/// - 终态 → `outcome=Terminal`
/// - 超时仍运行 → `outcome=StillRunning`
/// - 取消 → `outcome=Cancelled`（带最后已知 detail）
///
/// 等待期间若提供 `progress`，从**当前日志末尾**增量 `read_task_log` 并
/// 经 `ToolProgressEvent` 流式推送（NEVER 回放历史全文）。
/// **NEVER** 修改完成通知 / reminder 路径；本函数只读账本。
pub(crate) async fn wait_task(
    access: &dyn BackgroundProcessAccess,
    task_id: &str,
    timeout: Duration,
    cancellation: Arc<dyn CancellationSignal>,
    progress: Option<&dyn ProgressSink>,
) -> Result<BackgroundProcessWatchResult, String> {
    let deadline = Instant::now() + timeout;
    let poll = Duration::from_millis(WATCH_POLL_INTERVAL_MS);
    // cursor=None + max_bytes=0 → 停在当前文件末尾，不回放历史。
    let mut log_cursor = access
        .read_task_log(task_id, None, 0)
        .map(|log| log.cursor)
        .unwrap_or(0);
    let mut pending_log = String::new();

    loop {
        let Some(detail) = access.task_status(task_id) else {
            return Err(format!("Task not found: {task_id}"));
        };

        emit_watch_log_progress(access, task_id, &mut log_cursor, &mut pending_log, progress);

        if is_background_process_terminal_state(&detail.summary.state) {
            flush_watch_log_pending(&mut pending_log, progress);
            return Ok(watch_result(
                BackgroundProcessWatchOutcome::Terminal,
                detail,
            ));
        }

        if cancellation.is_cancelled() {
            flush_watch_log_pending(&mut pending_log, progress);
            return Ok(watch_result(
                BackgroundProcessWatchOutcome::Cancelled,
                detail,
            ));
        }

        if Instant::now() >= deadline {
            flush_watch_log_pending(&mut pending_log, progress);
            return Ok(watch_result(
                BackgroundProcessWatchOutcome::StillRunning,
                detail,
            ));
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let sleep_for = if remaining < poll { remaining } else { poll };
        if sleep_for.is_zero() {
            flush_watch_log_pending(&mut pending_log, progress);
            return Ok(watch_result(
                BackgroundProcessWatchOutcome::StillRunning,
                detail,
            ));
        }

        tokio::select! {
            _ = cancellation.cancelled() => {
                emit_watch_log_progress(
                    access,
                    task_id,
                    &mut log_cursor,
                    &mut pending_log,
                    progress,
                );
                flush_watch_log_pending(&mut pending_log, progress);
                let detail = access.task_status(task_id).unwrap_or(detail);
                if is_background_process_terminal_state(&detail.summary.state) {
                    return Ok(watch_result(
                        BackgroundProcessWatchOutcome::Terminal,
                        detail,
                    ));
                }
                return Ok(watch_result(
                    BackgroundProcessWatchOutcome::Cancelled,
                    detail,
                ));
            }
            _ = tokio::time::sleep(sleep_for) => {}
        }
    }
}

fn watch_result(
    outcome: BackgroundProcessWatchOutcome,
    detail: BackgroundProcessDetailData,
) -> BackgroundProcessWatchResult {
    BackgroundProcessWatchResult {
        outcome,
        detail,
        log_tail: String::new(),
        log_start_offset: 0,
        log_file: None,
    }
}

fn emit_watch_log_progress(
    access: &dyn BackgroundProcessAccess,
    task_id: &str,
    log_cursor: &mut u64,
    pending_log: &mut String,
    progress: Option<&dyn ProgressSink>,
) {
    let Some(sink) = progress else {
        return;
    };
    let Some(log) = access.read_task_log(task_id, Some(*log_cursor), WATCH_STREAM_LOG_CHUNK_BYTES)
    else {
        return;
    };
    *log_cursor = log.cursor;
    if log.text.is_empty() {
        return;
    }
    pending_log.push_str(&log.text);
    drain_watch_log_lines(pending_log, sink);
}

fn drain_watch_log_lines(pending_log: &mut String, sink: &dyn ProgressSink) {
    while let Some((line, rest)) = pending_log.split_once('\n') {
        let text = format!("{line}\n");
        *pending_log = rest.to_string();
        if !text.trim().is_empty() {
            sink.emit_tool_stream(ToolProgressEvent { text });
        }
    }
}

fn flush_watch_log_pending(pending_log: &mut String, progress: Option<&dyn ProgressSink>) {
    let Some(sink) = progress else {
        pending_log.clear();
        return;
    };
    if pending_log.trim().is_empty() {
        pending_log.clear();
        return;
    }
    let text = std::mem::take(pending_log);
    sink.emit_tool_stream(ToolProgressEvent { text });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct ScriptedAccess {
        states: Mutex<Vec<&'static str>>,
        polls: AtomicUsize,
    }

    impl ScriptedAccess {
        fn new(states: Vec<&'static str>) -> Self {
            Self {
                states: Mutex::new(states),
                polls: AtomicUsize::new(0),
            }
        }

        fn next_state(&self) -> Option<&'static str> {
            let mut guard = self.states.lock().expect("states");
            self.polls.fetch_add(1, Ordering::SeqCst);
            if guard.is_empty() {
                return None;
            }
            if guard.len() == 1 {
                return Some(guard[0]);
            }
            Some(guard.remove(0))
        }
    }

    impl BackgroundProcessAccess for ScriptedAccess {
        fn list_tasks(&self) -> Vec<BackgroundProcessSummaryData> {
            Vec::new()
        }

        fn task_status(&self, task_id: &str) -> Option<BackgroundProcessDetailData> {
            let state = self.next_state()?;
            Some(BackgroundProcessDetailData {
                summary: BackgroundProcessSummaryData {
                    task_id: task_id.to_string(),
                    tool_name: "Bash".to_string(),
                    state: state.to_string(),
                    goal: "Bash · sleep".to_string(),
                    duration_ms: Some(10),
                },
                deadline_remaining_ms: None,
                total_written_bytes: 0,
            })
        }

        fn read_task_log(
            &self,
            _task_id: &str,
            _cursor: Option<u64>,
            _max_bytes: usize,
        ) -> Option<BackgroundProcessLogData> {
            None
        }

        fn stop_task(&self, _task_id: &str) -> Result<BackgroundProcessStopData, String> {
            Err("not used".to_string())
        }
    }

    struct FlagCancel {
        flag: AtomicBool,
    }

    #[async_trait::async_trait]
    impl CancellationSignal for FlagCancel {
        fn is_cancelled(&self) -> bool {
            self.flag.load(Ordering::SeqCst)
        }
        async fn cancelled(&self) {
            loop {
                if self.is_cancelled() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        fn child_signal(&self) -> Arc<dyn CancellationSignal> {
            Arc::new(FlagCancel {
                flag: AtomicBool::new(self.is_cancelled()),
            })
        }
    }

    #[tokio::test]
    async fn wait_task_returns_terminal_immediately_when_already_done() {
        let access = ScriptedAccess::new(vec!["succeeded"]);
        let cancel = Arc::new(FlagCancel {
            flag: AtomicBool::new(false),
        });
        let result = wait_task(&access, "task_1", Duration::from_secs(5), cancel, None)
            .await
            .expect("wait");
        assert_eq!(result.outcome, BackgroundProcessWatchOutcome::Terminal);
        assert_eq!(result.detail.summary.state, "succeeded");
        assert_eq!(access.polls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn wait_task_times_out_as_still_running() {
        let access = ScriptedAccess::new(vec!["backgrounded"]);
        let cancel = Arc::new(FlagCancel {
            flag: AtomicBool::new(false),
        });
        let result = wait_task(&access, "task_1", Duration::from_millis(50), cancel, None)
            .await
            .expect("wait");
        assert_eq!(result.outcome, BackgroundProcessWatchOutcome::StillRunning);
        assert_eq!(result.detail.summary.state, "backgrounded");
        assert!(access.polls.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn wait_task_unknown_id_errors() {
        let access = ScriptedAccess::new(vec![]);
        let cancel = Arc::new(FlagCancel {
            flag: AtomicBool::new(false),
        });
        let err = wait_task(&access, "missing", Duration::from_secs(1), cancel, None)
            .await
            .expect_err("missing");
        assert!(err.contains("Task not found"));
    }

    #[tokio::test]
    async fn wait_task_cancelled_returns_cancelled_outcome() {
        let access = ScriptedAccess::new(vec!["backgrounded"]);
        let cancel = Arc::new(FlagCancel {
            flag: AtomicBool::new(true),
        });
        let result = wait_task(&access, "task_1", Duration::from_secs(5), cancel, None)
            .await
            .expect("wait");
        assert_eq!(result.outcome, BackgroundProcessWatchOutcome::Cancelled);
    }

    #[tokio::test]
    async fn wait_task_observes_transition_to_terminal() {
        let access = ScriptedAccess::new(vec!["backgrounded", "succeeded"]);
        let cancel = Arc::new(FlagCancel {
            flag: AtomicBool::new(false),
        });
        let result = wait_task(&access, "task_1", Duration::from_secs(5), cancel, None)
            .await
            .expect("wait");
        assert_eq!(result.outcome, BackgroundProcessWatchOutcome::Terminal);
        assert_eq!(result.detail.summary.state, "succeeded");
    }

    struct VecProgressSink {
        events: Mutex<Vec<String>>,
    }

    impl ProgressSink for VecProgressSink {
        fn emit(&self, _event: crate::domain::AgentProgressEvent) {}
        fn emit_tool_stream(&self, event: ToolProgressEvent) {
            self.events.lock().expect("events").push(event.text);
        }
    }

    /// 每次 `task_status` 从队列头部追加一段 log，供流式增量断言。
    struct FrontAppendLogAccess {
        states: Mutex<Vec<&'static str>>,
        log_bytes: Mutex<Vec<u8>>,
        append_on_poll: Mutex<Vec<&'static str>>,
    }

    impl FrontAppendLogAccess {
        fn new(
            states: Vec<&'static str>,
            initial_log: Vec<u8>,
            append_on_poll: Vec<&'static str>,
        ) -> Self {
            Self {
                states: Mutex::new(states),
                log_bytes: Mutex::new(initial_log),
                append_on_poll: Mutex::new(append_on_poll),
            }
        }
    }

    impl BackgroundProcessAccess for FrontAppendLogAccess {
        fn list_tasks(&self) -> Vec<BackgroundProcessSummaryData> {
            Vec::new()
        }

        fn task_status(&self, task_id: &str) -> Option<BackgroundProcessDetailData> {
            let mut states = self.states.lock().expect("states");
            if states.is_empty() {
                return None;
            }
            let state = if states.len() == 1 {
                states[0]
            } else {
                states.remove(0)
            };
            {
                let mut append = self.append_on_poll.lock().expect("append");
                if !append.is_empty() {
                    let chunk = append.remove(0);
                    self.log_bytes
                        .lock()
                        .expect("log")
                        .extend_from_slice(chunk.as_bytes());
                }
            }
            let total = self.log_bytes.lock().expect("log").len() as u64;
            Some(BackgroundProcessDetailData {
                summary: BackgroundProcessSummaryData {
                    task_id: task_id.to_string(),
                    tool_name: "Bash".to_string(),
                    state: state.to_string(),
                    goal: "Bash · ping".to_string(),
                    duration_ms: Some(10),
                },
                deadline_remaining_ms: None,
                total_written_bytes: total,
            })
        }

        fn read_task_log(
            &self,
            _task_id: &str,
            cursor: Option<u64>,
            max_bytes: usize,
        ) -> Option<BackgroundProcessLogData> {
            let bytes = self.log_bytes.lock().expect("log");
            let size = bytes.len() as u64;
            let start = cursor.unwrap_or(size.saturating_sub(max_bytes as u64));
            let start = start.min(size) as usize;
            let end = (start + max_bytes).min(bytes.len());
            let slice = bytes.get(start..end).unwrap_or(&[]);
            let text = String::from_utf8_lossy(slice).into_owned();
            Some(BackgroundProcessLogData {
                text,
                cursor: end as u64,
                total_written: size,
                start_offset: start as u64,
                is_tail: cursor.is_none(),
                log_file: None,
            })
        }

        fn stop_task(&self, _task_id: &str) -> Result<BackgroundProcessStopData, String> {
            Err("not used".to_string())
        }
    }

    #[tokio::test]
    async fn wait_task_emits_tool_progress_for_incremental_log_only() {
        let access = FrontAppendLogAccess::new(
            vec!["backgrounded", "backgrounded", "succeeded"],
            b"history-should-not-stream\n".to_vec(),
            vec!["stream-line-1\n", "stream-line-2\n"],
        );
        let sink = VecProgressSink {
            events: Mutex::new(Vec::new()),
        };
        let cancel = Arc::new(FlagCancel {
            flag: AtomicBool::new(false),
        });
        let result = wait_task(
            &access,
            "task_1",
            Duration::from_secs(5),
            cancel,
            Some(&sink),
        )
        .await
        .expect("wait");
        assert_eq!(result.outcome, BackgroundProcessWatchOutcome::Terminal);
        let events = sink.events.lock().expect("events").clone();
        let joined = events.join("");
        assert!(
            joined.contains("stream-line-1"),
            "应流式推送增量日志：{joined:?}"
        );
        assert!(
            joined.contains("stream-line-2"),
            "应流式推送后续增量：{joined:?}"
        );
        assert!(
            !joined.contains("history-should-not-stream"),
            "NEVER 回放历史日志：{joined:?}"
        );
    }
}
