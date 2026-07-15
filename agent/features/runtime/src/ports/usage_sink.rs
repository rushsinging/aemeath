//! UsageSink — Audit BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! PL 类型细化由 #927 负责；此处只定义最小骨架。

use crate::domain::agent_run::RunId;

// ─── Published Language（最小骨架，#927 迁移到 audit crate） ───

/// 一次 LLM 调用的 usage 记录。
// TODO(#927): 迁移到 audit crate 并细化字段。
#[derive(Debug, Clone)]
pub struct UsageRecord {
    /// 所属 Run。
    pub run_id: RunId,
    /// 模型名称。
    pub model: String,
    /// 输入 token 数。
    pub input_tokens: u64,
    /// 输出 token 数。
    pub output_tokens: u64,
    /// 缓存命中 token 数（如果有）。
    pub cached_tokens: u64,
}

/// Usage 提交结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageEmitOutcome {
    /// 记录已被接受。
    Accepted,
    /// 缓冲区已满，记录被丢弃（非阻塞语义）。
    Dropped,
}

// ─── Port trait ───

/// Audit BC 的出站端口（MVP 非阻塞 Pub/Sub）。
///
/// `try_record` 是非阻塞的——失败不改变 Run 状态。
/// Audit 只记录 Usage metadata；Cost/Pricing 在 v0.1.0 不进入 MVP。
pub trait UsageSink: Send + Sync {
    /// 非阻塞提交 usage 记录。
    fn try_record(&self, record: UsageRecord) -> UsageEmitOutcome;
}
