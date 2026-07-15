//! PolicyPort — Policy BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! PL 类型细化由 #917 负责；此处只定义最小骨架。

// ─── Published Language（最小骨架，#917 迁移到 policy crate） ───

/// 权限评估请求。
// TODO(#917): 迁移到 policy crate 并细化字段。
#[derive(Debug, Clone)]
pub struct PolicyRequest {
    /// 要执行的操作名称（如 tool name）。
    pub action: String,
    /// 操作参数。
    pub input: serde_json::Value,
}

/// 权限评估结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// 允许执行。
    Allow,
    /// 阻止执行。
    Deny { reason: String },
    /// 需要用户审批。
    RequireApproval { reason: String },
}

// ─── Port trait ───

/// Policy BC 的出站端口。
///
/// v0.1.0 唯一生产实现为 `AllowAllPolicy`（CLI `--yolo` / `--allow-all`）。
/// Future: Deny/RequireApproval 引擎。
pub trait PolicyPort: Send + Sync {
    /// 评估操作是否允许执行。
    fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision;
}
