//! ContextPort — Context Management BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! PL 类型细化由 #868 负责；此处只定义最小骨架。

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

// ─── Published Language（最小骨架，#868 迁移到 context crate） ───

/// 构建上下文窗口的请求。
// TODO(#868): 迁移到 context crate 并细化字段。
#[derive(Debug, Clone)]
pub struct ContextRequest {
    /// 用户本轮输入文本。
    pub user_input: String,
}

/// 构建完成的上下文窗口——包含历史消息、compact 摘要、memory 注入和 prompt 装配。
// TODO(#868): 迁移到 context crate 并细化字段。
#[derive(Debug, Clone)]
pub struct ContextWindow {
    /// 序列化后供 Provider 调用的消息列表。
    pub messages: Vec<share::message::Message>,
}

/// auto-compact 判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionDecision {
    /// 不需要压缩。
    NotNeeded,
    /// 需要执行 L5 auto-compact。
    Needed,
}

/// compact 执行结果。
// TODO(#868): 迁移到 context crate 并细化字段。
#[derive(Debug, Clone)]
pub struct CompactResult {
    /// 是否成功完成压缩。
    pub success: bool,
}

// ─── Port trait ───

/// Context Management BC 的出站端口。
///
/// Runtime 的 context_coordination 模块通过此端口：
/// - 构建本轮 Context Window（L2 snip + L3 microcompact + L4 collapse + memory 注入 + prompt 组装）
/// - 判断是否需要 auto-compact
/// - 执行 L5 auto-compact（LLM 摘要替换历史，唯一修改 ChatChain 的压缩策略）
#[async_trait(?Send)]
pub trait ContextPort: Send + Sync {
    /// 构建本轮 Context Window（只读变换，不修改 ChatChain）。
    fn build_window(&self, req: &ContextRequest) -> ContextWindow;

    /// 判断是否需要 auto-compact（幂等）。
    fn needs_compaction(&self, req: &ContextRequest) -> CompactionDecision;

    /// L5 auto-compact：LLM 摘要替换历史（唯一修改 ChatChain 的压缩策略）。
    async fn compact(
        &self,
        chain: &mut context::session::ChatChain,
        req: &ContextRequest,
        cancellation: &CancellationToken,
    ) -> CompactResult;
}
