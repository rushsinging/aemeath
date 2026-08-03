//! HookPort — Hook BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §2。
//! 一个类型化端口——Sub Run 使用 `BoundaryOnly`（仅 start/stop），过滤由 point metadata 完成。

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::domain::{HookInvocation, HookOutcome, HookPoint};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookSubscriptionExecutionTerminal {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookSubscriptionExecutionEvent {
    Started {
        point: HookPoint,
        script: String,
        attempt: u8,
    },
    AttemptChanged {
        point: HookPoint,
        script: String,
        attempt: u8,
    },
    Finished {
        point: HookPoint,
        script: String,
        terminal: HookSubscriptionExecutionTerminal,
    },
}

pub trait HookSubscriptionExecutionObserver: Send + Sync {
    fn observe(&self, event: HookSubscriptionExecutionEvent);
}

/// Hook 一次 dispatch 的工作区上下文。
///
/// Runtime 每次调用只提供当前 Workspace 的 cwd；Hook adapter 根据当前 invocation
/// 生成兼容环境变量并执行环境隔离。生命周期 observer 只报告 typed subscription 事实。
#[derive(Clone)]
pub struct HookDispatchContext {
    cwd: PathBuf,
    subscription_execution_observer: Option<std::sync::Arc<dyn HookSubscriptionExecutionObserver>>,
}

impl HookDispatchContext {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            subscription_execution_observer: None,
        }
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
pub trait HookPort: Send + Sync {
    /// 分发 hook 调用。
    ///
    /// `cancellation` 用于终止 Hook 子进程及重试等待。
    async fn dispatch(
        &self,
        invocation: HookInvocation,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcome;

    /// 使用当前工作区上下文分发 Hook。
    ///
    /// 默认实现保留给不依赖 workspace 的测试 fake；生产 Dispatcher 必须覆写，
    /// 以避免 worktree 切换后复用陈旧 cwd。
    async fn dispatch_at(
        &self,
        invocation: HookInvocation,
        _context: HookDispatchContext,
        cancellation: &dyn CancellationSignal,
    ) -> HookOutcome {
        self.dispatch(invocation, cancellation).await
    }
}
