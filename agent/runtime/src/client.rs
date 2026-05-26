//! AgentClient 实现 — RuntimeHandle 的薄代理。
//!
//! AgentClient trait 定义在 `packages/sdk`，此处提供具体实现。
//! `new()` 在 Phase 1 吞掉 setup.rs 的全部 build_* 逻辑。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use sdk::{
    AgentClient, ChangeSet, ChatInput, ChatStream, CostInfo, ProjectContext, SessionSnapshot,
    SdkError, TaskSummary,
};
use tokio::sync::{watch, RwLock};

use crate::api::core::task::TaskStore;

/// AgentClient 的 runtime 实现。
///
/// 持有 `RuntimeHandle`，所有方法都是委托调用。
/// Phase 0 先提供骨架实现，Phase 1 完善 new() 的初始化编排。
#[derive(Clone)]
pub struct AgentClientImpl {
    inner: Arc<RuntimeHandle>,
}

/// Runtime 内部状态句柄。
///
/// 被 AgentClientImpl 持有，封装 session、cost、task 等运行时状态。
pub struct RuntimeHandle {
    task_store: Arc<TaskStore>,
    cancel_token: Arc<AtomicBool>,
    change_tx: watch::Sender<ChangeSet>,
    change_rx: watch::Receiver<ChangeSet>,
}

impl RuntimeHandle {
    pub fn new(task_store: Arc<TaskStore>) -> Self {
        let (change_tx, change_rx) = watch::channel(ChangeSet::empty());
        Self {
            task_store,
            cancel_token: Arc::new(AtomicBool::new(false)),
            change_tx,
            change_rx,
        }
    }

    pub fn notify_change(&self, set: ChangeSet) {
        let _ = self.change_tx.send(set);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.load(Ordering::Acquire)
    }
}

impl AgentClientImpl {
    /// 创建 AgentClient 实例。
    ///
    /// Phase 1 将在此方法中完成全部初始化编排（替代 setup.rs）。
    /// Phase 0 先提供最小骨架。
    pub fn new(task_store: Arc<TaskStore>) -> Self {
        Self {
            inner: Arc::new(RuntimeHandle::new(task_store)),
        }
    }
}

#[async_trait]
impl AgentClient for AgentClientImpl {
    fn session_snapshot(&self) -> SessionSnapshot {
        // Phase 1: 从实际 session 获取
        SessionSnapshot {
            id: String::new(),
            message_count: 0,
            total_tokens: 0,
        }
    }

    fn cost(&self) -> CostInfo {
        // Phase 1: 从 cost_tracker 获取
        CostInfo::default()
    }

    fn task_list(&self) -> Vec<TaskSummary> {
        // Phase 1: 从 task_store 获取
        Vec::new()
    }

    fn project(&self) -> ProjectContext {
        // Phase 1: 从 project context 获取
        ProjectContext::default()
    }

    fn changes(&self) -> watch::Receiver<ChangeSet> {
        self.inner.change_rx.clone()
    }

    async fn chat(&self, _input: ChatInput) -> Result<ChatStream, SdkError> {
        // Phase 1: 连接到实际 chat loop
        let (_, rx) = tokio::sync::mpsc::unbounded_channel();
        Ok(ChatStream::new(rx))
    }

    fn cancel(&self) {
        self.inner.cancel_token.store(true, Ordering::Release);
    }

    async fn save_session(&self) -> Result<(), SdkError> {
        // Phase 1: 实际保存
        Ok(())
    }

    async fn load_session(&self, _id: &str) -> Result<SessionSnapshot, SdkError> {
        // Phase 1: 实际加载
        Ok(SessionSnapshot {
            id: _id.to_string(),
            message_count: 0,
            total_tokens: 0,
        })
    }

    async fn list_sessions(&self) -> Result<Vec<sdk::session::SessionSummary>, SdkError> {
        // Phase 1: 实际列表
        Ok(Vec::new())
    }

    async fn delete_session(&self, _id: &str) -> Result<(), SdkError> {
        // Phase 1: 实际删除
        Ok(())
    }

    async fn compact(&self) -> Result<(), SdkError> {
        // Phase 1: 实际压缩
        Ok(())
    }
}
