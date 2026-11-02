//! 会话恢复的窄入口（#2025）：供 Runtime 侧 Session 切换协议消费。
//!
//! `TaskPersist` 及 `PreparedTaskRestoreData` 对 runtime/tools 不可见
//! （F-12 架构守卫）；本 port 把两阶段恢复协议（prepare → commit）收敛为
//! TaskData BC 自有契约，ticket 类型不透明、不可构造、不可克隆。

use std::sync::Arc;

use share::error::DomainError;

use super::snapshot::{PreparedTaskRestoreData, TaskSnapshotData};
use super::task_persist::TaskPersist;

/// 两阶段恢复的不透明票券：prepare 产出、commit 消费，仅可提交一次。
pub struct SessionRestoreTicket(PreparedTaskRestoreData);

/// 会话恢复协议参与方契约（Runtime Session 切换协议消费面）。
pub trait TaskRestoreAccess: crate::sealed::Sealed + Send + Sync {
    /// 校验候选快照并预备恢复，不触碰活聚合；`None` 表示清空语义
    /// （等价 `TaskSnapshotData::empty()`）。
    fn prepare_session_restore(
        &self,
        snapshot: Option<&TaskSnapshotData>,
    ) -> Result<SessionRestoreTicket, DomainError>;

    /// 安装已预备的候选，整体替换聚合（不可失败）。
    fn commit_session_restore(&self, ticket: SessionRestoreTicket);
}

/// `TaskPersist` 的 `TaskRestoreAccess` 适配器（composition 注入用）。
pub struct TaskRestoreAccessAdapter {
    persist: Arc<dyn TaskPersist>,
}

impl TaskRestoreAccessAdapter {
    pub fn new(persist: Arc<dyn TaskPersist>) -> Self {
        Self { persist }
    }
}

impl TaskRestoreAccess for TaskRestoreAccessAdapter {
    fn prepare_session_restore(
        &self,
        snapshot: Option<&TaskSnapshotData>,
    ) -> Result<SessionRestoreTicket, DomainError> {
        match snapshot {
            Some(snapshot) => self.persist.prepare_restore(snapshot),
            None => self.persist.prepare_restore(&TaskSnapshotData::empty()),
        }
        .map(SessionRestoreTicket)
    }

    fn commit_session_restore(&self, ticket: SessionRestoreTicket) {
        self.persist.commit_restore(ticket.0);
    }
}

// #2146 密封解锁：实现权归属声明（Sealed 于定义 crate 内授予）。
impl crate::sealed::Sealed for TaskRestoreAccessAdapter {}
