//! UsageSink — Runtime-owned Audit 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2.3。
//! Usage Published Language 由 Audit BC 拥有；Runtime 只定义非阻塞提交对话。

pub use audit::{UsageDropReason, UsageEmitOutcome, UsageRecord};

/// Runtime-owned Audit 出站端口（MVP 非阻塞 Pub/Sub）。
///
/// `try_record` 是非阻塞的——失败不改变 Run 状态。
/// Audit 只记录 Usage metadata；Cost/Pricing 在 v0.1.0 不进入 MVP。
pub trait UsageSink: Send + Sync {
    /// 非阻塞提交 usage 记录。
    fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome;
}
