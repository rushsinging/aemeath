//! Typed input and result types for the background task tool family（#252）.

use serde::{Deserialize, Serialize};

/// `BackgroundTaskList` input（无参数：列出活动与近期任务）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackgroundTaskListInput {}

/// `BackgroundTaskStatus` input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskStatusInput {
    /// Task id（`task-` 前缀）。
    pub task_id: String,
}

/// `BackgroundTaskLogs` input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskLogsInput {
    /// Task id（`task-` 前缀）。
    pub task_id: String,
    /// 增量读取游标（上次读取返回的 cursor）；缺省读尾部。
    pub cursor: Option<u64>,
    /// 尾部/增量最大字节数（默认 4096）。
    pub max_bytes: Option<u64>,
}

/// `BackgroundTaskStop` input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskStopInput {
    /// Task id（`task-` 前缀）。
    pub task_id: String,
}

/// 后台任务摘要（list 条目）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskSummaryData {
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

/// `BackgroundTaskList` result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BackgroundTaskListResult {
    pub tasks: Vec<BackgroundTaskSummaryData>,
}

/// 单任务详情。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskDetailData {
    pub summary: BackgroundTaskSummaryData,
    /// deadline 快照剩余毫秒（无快照为 None）。
    pub deadline_remaining_ms: Option<u64>,
    /// 日志累计写入字节数（增量游标总坐标）。
    pub total_written_bytes: u64,
}

/// `BackgroundTaskStatus` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundTaskStatusResult {
    pub detail: BackgroundTaskDetailData,
}

/// 日志读取块（增量游标语义，多次读取幂等）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskLogData {
    /// 本次读到的文本（lossy 渲染）。
    pub text: String,
    /// 读后游标（下次携带只读新增）。
    pub cursor: u64,
    /// 累计写入字节（游标总坐标）。
    pub total_written: u64,
}

/// `BackgroundTaskLogs` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundTaskLogsResult {
    pub log: BackgroundTaskLogData,
}

/// stop 请求结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskStopData {
    /// true = 已发 cancel 信号；false = 任务已是终态（幂等）。
    pub signal_sent: bool,
    /// 当前状态词汇（终态词汇或 backgrounded/foreground_waiting）。
    pub state: String,
}

/// `BackgroundTaskStop` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskStopResult {
    pub task_id: String,
    pub stop: BackgroundTaskStopData,
}
