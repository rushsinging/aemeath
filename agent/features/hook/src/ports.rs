//! HookDispatcher — Hook BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §2。
//! 一个类型化端口——Main 使用 Full；Sub Run 使用 `BoundaryOnly`，过滤由
//! Hook-owned `HookPointMetadata.class` 完成并保留 Stop 与生命周期 Boundary。

#[cfg(test)]
#[path = "ports_tests.rs"]
mod tests;

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::domain::{HookInvocationData, HookOutcomeData, HookPointData};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookSubscriptionExecutionTerminalData {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookSubscriptionExecutionEventData {
    Started {
        point: HookPointData,
        script: String,
        attempt: u8,
    },
    AttemptChanged {
        point: HookPointData,
        script: String,
        attempt: u8,
    },
    Finished {
        point: HookPointData,
        script: String,
        terminal: HookSubscriptionExecutionTerminalData,
    },
}

pub trait HookSubscriptionExecutionObserver: Send + Sync {
    fn observe(&self, event: HookSubscriptionExecutionEventData);
}

/// Hook 一次 dispatch 的工作区上下文。
///
/// Runtime 每次调用提供当前 Workspace 的 cwd 与 Main Session id；Hook adapter
/// 根据当前 invocation 生成兼容环境变量并执行环境隔离。session_id 经
/// `AEMEATH_SESSION_ID` 注入 hook 子进程，供外部集成（如终端会话恢复工具）
/// 捕获当前会话。生命周期 observer 只报告 typed subscription 事实。
#[derive(Clone)]
pub struct HookDispatchContextData {
    cwd: PathBuf,
    session_id: Option<String>,
    subscription_execution_observer: Option<std::sync::Arc<dyn HookSubscriptionExecutionObserver>>,
}

impl HookDispatchContextData {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            session_id: None,
            subscription_execution_observer: None,
        }
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_subscription_execution_observer(
        mut self,
        observer: std::sync::Arc<dyn HookSubscriptionExecutionObserver>,
    ) -> Self {
        self.subscription_execution_observer = Some(observer);
        self
    }

    pub fn subscription_execution_observer(
        &self,
    ) -> Option<&std::sync::Arc<dyn HookSubscriptionExecutionObserver>> {
        self.subscription_execution_observer.as_ref()
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
}

/// Hook domain 所需的最小协作取消能力。
#[async_trait]
pub trait CancellationSignal: Send + Sync {
    fn is_cancelled(&self) -> bool;
    async fn cancelled(&self);
}

#[async_trait]
impl CancellationSignal for tokio_util::sync::CancellationToken {
    fn is_cancelled(&self) -> bool {
        tokio_util::sync::CancellationToken::is_cancelled(self)
    }

    async fn cancelled(&self) {
        tokio_util::sync::CancellationToken::cancelled(self).await;
    }
}

/// Hook BC 的出站端口。
///
/// 协议固定：
/// - 任意非零 exit 是主动 Block，不因 exit code 重试；
/// - 仅 spawn/wait/IO/timeout/非法 JSON 等 ExecutionFailed 重试。
#[async_trait]
pub trait HookDispatcher: Send + Sync {
    /// 分发 hook 调用。
    ///
    /// `cancellation` 用于终止 Hook 子进程及重试等待。
    async fn dispatch(
        &self,
        invocation: HookInvocationData,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcomeData;

    /// 使用当前工作区上下文分发 Hook。
    ///
    /// 默认实现保留给不依赖 workspace 的测试 fake；生产 Dispatcher 必须覆写，
    /// 以避免 worktree 切换后复用陈旧 cwd。
    async fn dispatch_at(
        &self,
        invocation: HookInvocationData,
        _context: HookDispatchContextData,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcomeData {
        self.dispatch(invocation, cancellation).await
    }
}
