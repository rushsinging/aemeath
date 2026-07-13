//! HookPort — Hook BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! PL 类型细化由 #922 负责；此处只定义最小骨架。

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

// ─── Published Language（最小骨架，#922 迁移到 hook crate） ───

/// Hook 触发点——Runtime 经此通知 Hook BC 在特定时机执行。
///
/// 协议固定：任意非零 exit 是主动 Block，不因 exit code 重试；
/// 仅 spawn/wait/IO/timeout/非法 JSON 等 ExecutionFailed 重试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPoint {
    /// 用户提交 prompt 后、调用模型前。
    UserPromptSubmit,
    /// 模型返回 stop、且本轮不会再调用模型。
    Stop,
    /// 工具执行前。
    PreToolCall,
    /// 工具执行后。
    PostToolCall,
    /// Sub Run 启动。
    SubRunStart,
    /// Sub Run 终止。
    SubRunStop,
    /// 通知。
    Notification,
}

/// Hook 调用请求。
// TODO(#922): 迁移到 hook crate 并细化字段。
#[derive(Debug, Clone)]
pub struct HookInvocation {
    /// 触发点。
    pub point: HookPoint,
    /// 传递给 hook 脚本的载荷（JSON）。
    pub payload: serde_json::Value,
}

/// Hook 执行结果。
///
/// Runtime 拥有 directive 响应编排（如 Stop 阻断累计 15 次后第 16 次 RunFailed）。
#[derive(Debug, Clone)]
pub enum HookOutcome {
    /// Hook 允许继续。
    Proceed,
    /// Hook 主动阻断（任意非零 exit code）。
    Block { reason: String },
    /// Hook 执行失败（spawn/wait/IO/timeout/非法 JSON）。
    ExecutionFailed { error: String },
}

// ─── Port trait ───

/// Hook BC 的出站端口——一个类型化端口。
///
/// Sub Run 使用 `BoundaryOnly`（仅 start/stop），过滤由 point metadata 完成。
#[async_trait]
pub trait HookPort: Send + Sync {
    /// 分发 hook 调用。
    async fn dispatch(
        &self,
        invocation: HookInvocation,
        cancellation: &CancellationToken,
    ) -> HookOutcome;
}
