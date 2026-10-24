//! Tools adapters 层共享生产常量（#1146 双轨归位）。

use std::time::Duration;

pub(crate) const SUB_AGENT_DEFAULT_TIMEOUT_SECS: u64 = 3600;

/// WebFetch `timeout` 字段单位：秒（硬切，不再接受毫秒）。网络单请求
/// 不需要 30 分钟默认预算，故覆写工具默认值；上限统一走
/// [`crate::domain::constants::DEFAULT_TOOL_MAX_TIMEOUT_SECS`]。
pub(crate) const WEB_FETCH_DEFAULT_TIMEOUT_SECS: u64 = 30;

pub(crate) const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

pub(crate) const TERM_GRACE: Duration = Duration::from_millis(200);

pub(crate) const BUILTIN_COMMIT_URI: &str = "aemeath-builtin://commit";

/// `BackgroundProcesss` logs action 缺省尾部/增量读取字节数（#252）。
pub(crate) const DEFAULT_BACKGROUND_LOG_MAX_BYTES: usize = 4096;

/// `BackgroundProcessWatch` 终态 tool_result 文本字符预算。
///
/// **为何设硬顶**：任务日志本已落盘；若再让整段 log 进 LLM text 并触
/// materializer 二次落盘，属于扩散。硬压在 materializer floor（3000）之下。
/// 对环境工具（Grep/Bash/Read 等）**NEVER** 套用同源头硬顶。
pub(crate) const WATCH_RESULT_TEXT_BUDGET_CHARS: usize = 2_000;

/// Watch 摘要行预留字符；剩余预算给 log_tail。
pub(crate) const WATCH_RESULT_HEADER_RESERVE_CHARS: usize = 260;

/// Memory List/Search/eviction 的 tool_result 文本字符预算。
///
/// **为何设硬顶**：Memory 是有预期体量的内部调用（分页/检索），源头裁到可控
/// 摘要；同类还包括 Skill / 内部 Search 等。对环境工具 **NEVER** 设源头硬顶。
pub(crate) const MEMORY_RESULT_TEXT_BUDGET_CHARS: usize = 2_000;

/// Memory 结果摘要行预留字符；剩余预算给逐条精简行。
pub(crate) const MEMORY_RESULT_HEADER_RESERVE_CHARS: usize = 260;

/// Task not-found 空列表引导语（#1990）：点破「列表 ≠ 任务」。
pub(crate) const TASK_LIST_NOT_TASK_HINT: &str =
    "TaskListCreate creates a list, not a task; add tasks with TaskCreate";
