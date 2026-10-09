//! 后台进程查询端口（#252）：tools 侧契约，runtime 提供实现。

use std::sync::Arc;

use super::types::background_processes::{
    BackgroundProcessDetailData, BackgroundProcessLogData, BackgroundProcessStopData,
    BackgroundProcessSummaryData,
};

/// 后台进程账本读取与停止请求端口。
///
/// 实现方持有 session 级监督器；所有方法只读或幂等信号，
/// NEVER 阻塞等待任务真实终态。
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
