//! Typed input and result types for the `BackgroundTasks` tool（#252）。

use serde::{Deserialize, Serialize};

/// 查询动作（单一 tool 多 action，先例 MemoryStatus）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTasksAction {
    /// 列出活动与近期后台任务（id / 工具 / 状态 / 时长）。
    List,
    /// 单任务详情：状态、终态、deadline 剩余。
    Status,
    /// 查询任务日志（运行中与完成后皆可）：尾部或增量游标读取。
    Logs,
    /// 请求停止：发 cancel 信号，真实终态由执行体收口。
    Stop,
}

/// BackgroundTasks input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTasksInput {
    /// Action: `list` | `status` | `logs` | `stop`.
    pub action: BackgroundTasksAction,
    /// Task id（`task-` 前缀），`status` / `logs` / `stop` 必填。
    pub task_id: Option<String>,
    /// `logs`：增量读取游标（上次 read_log 返回的 cursor）；缺省读尾部。
    pub cursor: Option<u64>,
    /// `logs`：尾部/增量最大字节数（默认 4096）。
    pub max_bytes: Option<u64>,
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

/// 单任务详情。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskDetailData {
    pub summary: BackgroundTaskSummaryData,
    /// deadline 快照剩余毫秒（无快照为 None）。
    pub deadline_remaining_ms: Option<u64>,
    /// 日志累计写入字节数（增量游标总坐标）。
    pub total_written_bytes: u64,
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

/// stop 请求结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskStopData {
    /// true = 已发 cancel 信号；false = 任务已是终态（幂等）。
    pub signal_sent: bool,
    /// 当前状态词汇（终态词汇或 backgrounded/foreground_waiting）。
    pub state: String,
}

/// BackgroundTasks result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BackgroundTasksResult {
    pub action: Option<String>,
    pub tasks: Vec<BackgroundTaskSummaryData>,
    pub detail: Option<BackgroundTaskDetailData>,
    pub log: Option<BackgroundTaskLogData>,
    pub stop: Option<BackgroundTaskStopData>,
}
