//! HookPort — Hook BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/hook/README.md` §2。
//! 一个类型化端口——Sub Run 使用 `BoundaryOnly`（仅 start/stop），过滤由 point metadata 完成。

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::domain::{HookInvocation, HookOutcome};

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
}
