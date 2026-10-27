//! RSI 任务注册面：daemon 按 TaskKind 插拔改进任务（03 分片第 6 节）。
//!
//! 当前仅注册 `HarnessRsi`（reflect NEVER 进入 harness 进程——2026-10-22 拍板，
//! 纯组件治理单一职责）；插拔面保留给后续组件治理任务（如 c8 plugin）。

use crate::constants::ERROR_DOMAIN;
use crate::domain::errors::HarnessError;
use async_trait::async_trait;
use std::collections::HashMap;

/// 改进任务类别（注册面键）。
///
/// 当前仅 `HarnessRsi`（纯组件治理，单一职责——reflect NEVER 进入
/// harness 进程，2026-10-22 拍板）；插拔面保留给后续组件治理任务
/// （如 c8 plugin）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskKind {
    /// harness 组件改进（证据→提案→原子落盘）。
    HarnessRsi,
}

impl TaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HarnessRsi => "harness-rsi",
        }
    }
}

/// 单个改进周期的产出摘要（Status/日志呈现）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleOutcome {
    /// 人类可读结果摘要（如「采纳 2 / 作废 1 / 无变更 0」）。
    pub summary: String,
    /// 本周期采纳的编辑记录（Compose 汇总块的材料，#2065 A）。
    pub adopted: Vec<AdoptionRecord>,
}

/// 单条采纳记录（组件 + 提案依据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptionRecord {
    pub id: crate::domain::component::ComponentId,
    pub reason: String,
}

/// RSI 任务接口：一个完整改进周期（分块在实现内部组织）。
#[async_trait]
pub trait RsiTask: Send {
    /// 执行一轮改进周期。实现 MUST 可中断友好（周期内不阻塞控制面应答）。
    async fn run_cycle(&mut self) -> Result<CycleOutcome, HarnessError>;
}

/// 任务注册表：TaskKind → 任务实例，装配期注入。
#[derive(Default)]
pub struct TaskRegistry {
    tasks: HashMap<TaskKind, Box<dyn RsiTask>>,
}

impl TaskRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册任务；重复注册同 kind 为 Invalid（装配错误早暴露）。
    pub fn register(&mut self, kind: TaskKind, task: Box<dyn RsiTask>) -> Result<(), HarnessError> {
        if self.tasks.contains_key(&kind) {
            return Err(HarnessError::invalid(
                ERROR_DOMAIN,
                format!("任务重复注册：{}", kind.as_str()),
            ));
        }
        self.tasks.insert(kind, task);
        Ok(())
    }

    /// 依次执行全部注册任务的一轮周期（串行；单任务失败不中断其余任务，
    /// 失败计入汇总）。
    pub async fn run_all_cycles(&mut self) -> Vec<(TaskKind, Result<CycleOutcome, HarnessError>)> {
        let mut kinds: Vec<TaskKind> = self.tasks.keys().copied().collect();
        kinds.sort_by_key(|kind| kind.as_str());
        let mut results = Vec::with_capacity(kinds.len());
        for kind in kinds {
            if let Some(task) = self.tasks.get_mut(&kind) {
                let outcome = task.run_cycle().await;
                results.push((kind, outcome));
            }
        }
        results
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
}
