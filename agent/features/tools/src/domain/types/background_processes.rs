//! Typed input and result types for the background process tool family（#252 / #1931）.

use serde::{Deserialize, Serialize};

use crate::domain::constants::{WATCH_INTERNAL_GRACE_SECS, WATCH_TOOL_TIMEOUT_SECS};

/// `BackgroundProcessList` input（无参数：列出活动与近期任务）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackgroundProcessListInput {}

/// `BackgroundProcessStatus` input.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackgroundProcessStatusInput {
    /// Task id（`task-` 前缀）。
    pub task_id: String,
}

/// `BackgroundProcessLogs` input.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackgroundProcessLogsInput {
    /// Task id（`task-` 前缀）。
    pub task_id: String,
    /// 增量读取游标（上次读取返回的 cursor）；缺省读尾部。
    pub cursor: Option<u64>,
    /// 尾部/增量最大字节数（默认 4096）。
    pub max_bytes: Option<u64>,
}

/// `BackgroundProcessStop` input.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackgroundProcessStopInput {
    /// Task id（`task-` 前缀）。
    pub task_id: String,
}

/// 后台进程摘要（list 条目）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundProcessSummaryData {
    pub task_id: String,
    pub tool_name: String,
    /// 状态词汇：`foreground_waiting` | `backgrounded` | `succeeded` |
    /// `failed` | `timed_out` | `stopped` | `invalidated`。
    pub state: String,
    /// 摘要（工具名 + 输入预览）。
    pub summary: String,
    /// 运行时长（毫秒；终态后为终态耗时）。
    pub duration_ms: Option<u64>,
}

/// `BackgroundProcessList` result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BackgroundProcessListResult {
    pub tasks: Vec<BackgroundProcessSummaryData>,
}

/// 单任务详情。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundProcessDetailData {
    pub summary: BackgroundProcessSummaryData,
    /// deadline 快照剩余毫秒（无快照为 None）。
    pub deadline_remaining_ms: Option<u64>,
    /// 日志累计写入字节数（增量游标总坐标）。
    pub total_written_bytes: u64,
}

/// `BackgroundProcessStatus` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundProcessStatusResult {
    pub detail: BackgroundProcessDetailData,
}

/// 日志读取块（增量游标语义，多次读取幂等）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundProcessLogData {
    /// 本次读到的文本（lossy 渲染）。
    pub text: String,
    /// 读后游标（下次携带只读新增）。
    pub cursor: u64,
    /// 累计写入字节（游标总坐标）。
    pub total_written: u64,
}

/// `BackgroundProcessLogs` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundProcessLogsResult {
    pub log: BackgroundProcessLogData,
}

/// stop 请求结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundProcessStopData {
    /// true = 已发 cancel 信号；false = 任务已是终态（幂等）。
    pub signal_sent: bool,
    /// 当前状态词汇（终态词汇或 backgrounded/foreground_waiting）。
    pub state: String,
}

/// `BackgroundProcessStop` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundProcessStopResult {
    pub task_id: String,
    pub stop: BackgroundProcessStopData,
}

/// `BackgroundProcessWatch` input.
///
/// 超时唯一来源为 Tool 框架 [`WATCH_TOOL_TIMEOUT_SECS`]（#1946）；
/// **NEVER** 再提供入参 timeout。旧调用若仍带 `timeout_secs` 由 serde 忽略。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackgroundProcessWatchInput {
    /// Task id（`task_` / typed id 前缀）。
    pub task_id: String,
}

/// Watch 等待结局。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundProcessWatchOutcome {
    /// 在超时前观察到终态。
    Terminal,
    /// 超时仍未终态。
    StillRunning,
    /// 等待被 Run/tool cancellation 打断。
    Cancelled,
}

/// `BackgroundProcessWatch` result。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundProcessWatchResult {
    pub outcome: BackgroundProcessWatchOutcome,
    pub detail: BackgroundProcessDetailData,
    /// 给 LLM / TUI 的日志尾部（字符预算内）；缺省空串兼容旧快照。
    #[serde(default)]
    pub log_tail: String,
}

/// Watch 内部等待时长：框架超时减去 grace，先于 supervisor 收口为 `still_running`。
pub fn watch_internal_timeout_secs() -> u64 {
    WATCH_TOOL_TIMEOUT_SECS.saturating_sub(WATCH_INTERNAL_GRACE_SECS)
}

/// 状态词汇是否为后台进程终态（与 Status/List 词汇对齐）。
pub fn is_background_process_terminal_state(state: &str) -> bool {
    matches!(
        state,
        "succeeded" | "failed" | "timed_out" | "stopped" | "invalidated"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_detail(state: &str) -> BackgroundProcessDetailData {
        BackgroundProcessDetailData {
            summary: BackgroundProcessSummaryData {
                task_id: "task_01hxyz".to_string(),
                tool_name: "Bash".to_string(),
                state: state.to_string(),
                summary: "Bash · cargo test".to_string(),
                duration_ms: Some(1_200),
            },
            deadline_remaining_ms: None,
            total_written_bytes: 64,
        }
    }

    #[test]
    fn watch_timeouts_are_framework_only_with_internal_grace() {
        assert_eq!(WATCH_TOOL_TIMEOUT_SECS, 300);
        assert_eq!(WATCH_INTERNAL_GRACE_SECS, 5);
        assert_eq!(watch_internal_timeout_secs(), 295);
        assert!(
            watch_internal_timeout_secs() < WATCH_TOOL_TIMEOUT_SECS,
            "内部 wait MUST 短于框架 timeout，避免 cleanup unconfirmed"
        );
    }

    #[test]
    fn watch_input_ignores_legacy_timeout_secs_field() {
        let input: BackgroundProcessWatchInput = serde_json::from_value(serde_json::json!({
            "task_id": "bgp_legacy1",
            "timeout_secs": 300
        }))
        .expect("旧 timeout_secs 字段应被忽略");
        assert_eq!(input.task_id, "bgp_legacy1");
        let schema = {
            use crate::domain::types::ToolSchema;
            BackgroundProcessWatchInput::data_schema()
        };
        let schema_text = schema.to_string();
        assert!(
            !schema_text.contains("timeout_secs"),
            "Watch input schema MUST NOT 暴露 timeout_secs：{schema_text}"
        );
    }

    #[test]
    fn watch_result_serde_round_trip() {
        let result = BackgroundProcessWatchResult {
            outcome: BackgroundProcessWatchOutcome::StillRunning,
            detail: sample_detail("backgrounded"),
            log_tail: "tail-line\n".to_string(),
        };
        let json = serde_json::to_value(&result).expect("serialize");
        assert_eq!(json["outcome"], "still_running");
        assert_eq!(json["log_tail"], "tail-line\n");
        let back: BackgroundProcessWatchResult = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back.outcome, BackgroundProcessWatchOutcome::StillRunning);
        assert_eq!(back.detail.summary.state, "backgrounded");
        assert_eq!(back.log_tail, "tail-line\n");
        let legacy = serde_json::json!({
            "outcome": "terminal",
            "detail": sample_detail("succeeded"),
        });
        let from_legacy: BackgroundProcessWatchResult =
            serde_json::from_value(legacy).expect("legacy without log_tail");
        assert!(from_legacy.log_tail.is_empty());
    }

    #[test]
    fn terminal_state_helper_matches_status_vocabulary() {
        assert!(is_background_process_terminal_state("succeeded"));
        assert!(is_background_process_terminal_state("failed"));
        assert!(is_background_process_terminal_state("timed_out"));
        assert!(is_background_process_terminal_state("stopped"));
        assert!(is_background_process_terminal_state("invalidated"));
        assert!(!is_background_process_terminal_state("backgrounded"));
        assert!(!is_background_process_terminal_state("foreground_waiting"));
    }
}
