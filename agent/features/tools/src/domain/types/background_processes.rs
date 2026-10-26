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
    /// 调用意图（#1964 = ToolCall::goal_text / 账本 invocation_goal）。
    #[serde(alias = "summary")]
    pub goal: String,
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
/// 等待上限取框架解析的 effective `timeout`（`ctx.effective_timeout_secs()`），
/// 工具框架硬上限仍为 [`WATCH_TOOL_TIMEOUT_SECS`]（#1946）：入参与内部 wait 必须
/// 在该上限内收口，**NEVER** 让框架 deadline 先于 wait 砍成 cleanup unconfirmed。
/// `timeout` 是框架级通用参数，catalog 注入发布、执行前框架摘除，**MUST NOT**
/// 在此声明（本工具无自有字段）。
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

/// 解析 Watch 实际等待秒数（入参 = 统一字段 `timeout`）。
///
/// - 省略入参 → 默认框架上限；
/// - clamp 到 `1..=WATCH_TOOL_TIMEOUT_SECS`；
/// - 再与 `框架上限 − grace` 取 min，逼近框架窗时先收口为 `still_running`。
pub fn resolve_watch_wait_secs(timeout: Option<u64>) -> u64 {
    let requested = timeout
        .unwrap_or(WATCH_TOOL_TIMEOUT_SECS)
        .clamp(1, WATCH_TOOL_TIMEOUT_SECS);
    let max_safe = WATCH_TOOL_TIMEOUT_SECS.saturating_sub(WATCH_INTERNAL_GRACE_SECS);
    requested.min(max_safe).max(1)
}

/// 省略入参时的内部等待时长（兼容旧调用点）。
pub fn watch_internal_timeout_secs() -> u64 {
    resolve_watch_wait_secs(None)
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
                goal: "Bash · cargo test".to_string(),
                duration_ms: Some(1_200),
            },
            deadline_remaining_ms: None,
            total_written_bytes: 64,
        }
    }

    #[test]
    fn watch_timeouts_clamp_under_framework_cap_with_grace() {
        assert_eq!(WATCH_TOOL_TIMEOUT_SECS, 300);
        assert_eq!(WATCH_INTERNAL_GRACE_SECS, 5);
        assert_eq!(resolve_watch_wait_secs(None), 295);
        assert_eq!(resolve_watch_wait_secs(Some(60)), 60);
        assert_eq!(resolve_watch_wait_secs(Some(1)), 1);
        assert_eq!(resolve_watch_wait_secs(Some(0)), 1);
        assert_eq!(resolve_watch_wait_secs(Some(999)), 295);
        assert_eq!(resolve_watch_wait_secs(Some(300)), 295);
        assert!(
            resolve_watch_wait_secs(None) < WATCH_TOOL_TIMEOUT_SECS,
            "逼近框架上限时内部 wait MUST 短于框架 timeout，避免 cleanup unconfirmed"
        );
    }

    #[test]
    fn watch_input_keeps_only_task_id_and_schema_omits_framework_timeout() {
        // `timeout` 是框架级通用参数：框架在派发前摘除，工具 input 只承载自有字段。
        // 直接反序列化（旁路，未经框架摘除）时 MUST 容忍该字段而非报错。
        let input: BackgroundProcessWatchInput = serde_json::from_value(serde_json::json!({
            "task_id": "bgp_watch1",
            "timeout": 60
        }))
        .expect("含框架 timeout 的 input 必须可解析（旁路兼容）");
        assert_eq!(input.task_id, "bgp_watch1");

        let legacy: BackgroundProcessWatchInput = serde_json::from_value(serde_json::json!({
            "task_id": "bgp_legacy",
            "timeout_secs": 60
        }))
        .expect("旧 timeout_secs 字段名 MUST 被忽略");
        assert_eq!(legacy.task_id, "bgp_legacy");

        let schema = {
            use crate::domain::types::ToolSchema;
            BackgroundProcessWatchInput::data_schema()
        };
        let schema_text = schema.to_string();
        assert!(
            !schema_text.contains("timeout"),
            "工具 input schema MUST NOT 声明 timeout（由 catalog 统一注入）：{schema_text}"
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
