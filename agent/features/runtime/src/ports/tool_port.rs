//! ToolCatalogPort / ToolExecutionPort — Tool BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! PL 类型细化由 #908 负责；此处只定义最小骨架。

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

// ─── Published Language（最小骨架，#908 迁移到 tools crate） ───

/// Registry Scope 名称——决定装配哪些工具资源。
// TODO(#908): 迁移到 tools crate。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegistryScopeName(pub String);

/// Tool Profile 名称——决定 capability 集只能收缩。
// TODO(#908): 迁移到 tools crate。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolProfileName(pub String);

/// 只读目录投影——不暴露 Registry/Tool 实例。
// TODO(#908): 迁移到 tools crate 并细化字段。
#[derive(Debug, Clone)]
pub struct ToolCatalogSnapshot {
    /// 工具定义 JSON schema 列表（供 Provider 组装 tool 定义）。
    pub schemas: Vec<serde_json::Value>,
}

/// 一次工具执行调用。
// TODO(#908): 迁移到 tools crate 并细化字段。
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    /// 工具名称。
    pub name: String,
    /// 工具输入参数。
    pub input: serde_json::Value,
}

/// 工具执行结果。
// TODO(#908): 迁移到 tools crate 并细化字段。
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    /// 执行是否成功。
    pub success: bool,
    /// 结果内容。
    pub content: String,
}

// ─── Port traits ───

/// Tool BC 只读目录端口——按 Scope/Profile 投影 schemas。
pub trait ToolCatalogPort: Send + Sync {
    /// 返回指定 Scope ∩ Profile 下的工具目录快照。
    fn snapshot(&self, scope: &RegistryScopeName, profile: &ToolProfileName)
        -> ToolCatalogSnapshot;
}

/// Tool BC 单次函数调用端口——不暴露 Tool/Registry 实例。
#[async_trait]
pub trait ToolExecutionPort: Send + Sync {
    /// 执行单次工具调用。
    async fn execute(
        &self,
        invocation: ToolInvocation,
        cancellation: &CancellationToken,
    ) -> ToolOutcome;
}
