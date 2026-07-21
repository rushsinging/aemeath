//! HookPort — Hook BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §2。
//! 一个类型化端口——Sub Run 使用 `BoundaryOnly`（仅 start/stop），过滤由 point metadata 完成。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::domain::{HookInvocation, HookOutcome};

/// Hook 一次 dispatch 的运行时环境。
///
/// Runtime 每次调用从当前 Workspace 读取 cwd；Hook adapter 根据 invocation 派生
/// 兼容环境变量。环境清空及白名单策略由 #1216 收口。
#[derive(Debug, Clone)]
pub struct HookDispatchContext {
    cwd: PathBuf,
    env: HashMap<String, String>,
}

impl HookDispatchContext {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            env: HashMap::new(),
        }
    }

    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn env(&self) -> &HashMap<String, String> {
        &self.env
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
        cancellation: &CancellationToken,
    ) -> HookOutcome;

    /// 使用当前工作区上下文分发 Hook。
    ///
    /// 默认实现保留给不依赖 workspace 的测试 fake；生产 Dispatcher 必须覆写，
    /// 以避免 worktree 切换后复用陈旧 cwd。
    async fn dispatch_at(
        &self,
        invocation: HookInvocation,
        _context: HookDispatchContext,
        cancellation: &CancellationToken,
    ) -> HookOutcome {
        self.dispatch(invocation, cancellation).await
    }
}
