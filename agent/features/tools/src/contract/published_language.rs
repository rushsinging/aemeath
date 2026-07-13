//! Tool 领域 Published Language（DDD §6.4.3）。
//!
//! 本模块定义 Tool BC 的值对象和领域类型，供 [`crate::contract::ports`]
//! 双端口使用。消费方只依赖这些类型和端口 trait，不接触 `Tool` 实例、
//! `ToolRegistry`、MCP client 或函数指针。
//!
//! 设计来源：`docs/design/02-modules/tools/01-domain-model.md`。
//!
//! # 不变量
//!
//! - `ToolName` 在同一 Registry Scope 内唯一，规范化为 ASCII 小写；
//! - `ToolCapabilities` 只能通过 baseline 或 `derive_restricted` 收缩，不可扩权；
//! - `ToolOutcome` 是单一结果通道（含错误），不额外暴露 `Result::Err`。

use serde::{Deserialize, Serialize};
use std::fmt;

// ── ToolName ────────────────────────────────────────────────────────

/// 规范化逻辑键：在同一 Registry Scope 内唯一。
///
/// 规范化策略与 `ToolRegistry` 的 `normalize_key` 一致（ASCII 小写），
/// 保证注册与查找的 key 统一。MCP 限定名（`mcp__server__tool`）的
/// 跨段语义不受影响。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolName(String);

impl ToolName {
    /// 从任意字符串构造，内部存储规范化后的值。
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into().to_ascii_lowercase())
    }

    /// 规范化（ASCII 小写）后的名称。
    pub fn normalized(&self) -> &str {
        &self.0
    }

    /// 原始值（已规范化，等价于 `normalized`）。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ToolName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ToolName {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

// ── ToolCapability ──────────────────────────────────────────────────

/// 工具执行所需能力。Profile 声明允许能力。
///
/// Capability 表达安全权限，不表达 Tool 身份或装配位置。
/// 新增 Tool 未声明 required capabilities 时不得注册。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolCapability {
    /// 读取工作区（文件 / 目录）。
    ReadWorkspace,
    /// 写入工作区（创建 / 修改 / 删除文件）。
    WriteWorkspace,
    /// 执行外部进程（bash 等）。
    ExecuteProcess,
    /// 网络访问（web fetch / search 等）。
    NetworkAccess,
    /// 用户交互（AskUserQuestion 等）。
    UserInteraction,
    /// 派发子 agent。
    AgentDispatch,
    /// 修改 Task 列表。
    TaskMutation,
    /// 控制 workspace（worktree 进入 / 退出）。
    WorkspaceControl,
    /// 控制 plan mode。
    PlanControl,
}

// 能力集合（bitflags）。
//
// 用于 `ToolDescriptor::required_capabilities` 和 `ToolProfile::allowed_capabilities`。
// 有效工具集 = Registry Scope ∩ Profile Allowed Capabilities。
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct ToolCapabilities: u32 {
        const ReadWorkspace    = 1 << 0;
        const WriteWorkspace   = 1 << 1;
        const ExecuteProcess   = 1 << 2;
        const NetworkAccess    = 1 << 3;
        const UserInteraction  = 1 << 4;
        const AgentDispatch    = 1 << 5;
        const TaskMutation     = 1 << 6;
        const WorkspaceControl = 1 << 7;
        const PlanControl      = 1 << 8;
    }
}

impl ToolCapabilities {
    /// 从单个 capability 构造。
    pub fn single(cap: ToolCapability) -> Self {
        Self::from(cap)
    }

    /// 从多个 capability 构造。
    pub fn from_caps(caps: impl IntoIterator<Item = ToolCapability>) -> Self {
        caps.into_iter()
            .fold(Self::empty(), |acc, c| acc | Self::from(c))
    }

    /// 是否包含指定 capability。
    pub fn contains_cap(self, cap: ToolCapability) -> bool {
        self.contains(Self::from(cap))
    }

    /// `self` 是否是 `other` 的子集。
    pub fn is_subset_of(self, other: Self) -> bool {
        self.intersection(other) == self
    }
}

impl From<ToolCapability> for ToolCapabilities {
    fn from(cap: ToolCapability) -> Self {
        match cap {
            ToolCapability::ReadWorkspace => Self::ReadWorkspace,
            ToolCapability::WriteWorkspace => Self::WriteWorkspace,
            ToolCapability::ExecuteProcess => Self::ExecuteProcess,
            ToolCapability::NetworkAccess => Self::NetworkAccess,
            ToolCapability::UserInteraction => Self::UserInteraction,
            ToolCapability::AgentDispatch => Self::AgentDispatch,
            ToolCapability::TaskMutation => Self::TaskMutation,
            ToolCapability::WorkspaceControl => Self::WorkspaceControl,
            ToolCapability::PlanControl => Self::PlanControl,
        }
    }
}

// ── Concurrency / Cancellation ──────────────────────────────────────

/// 工具并发安全声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcurrencySafety {
    /// 可与其他 Safe Tool 并发。
    Safe,
    /// 同一 ToolName 全局串行。
    Serialized,
}

/// 工具并发声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConcurrencyDeclaration {
    pub safety: ConcurrencySafety,
}

impl ConcurrencyDeclaration {
    pub fn safe() -> Self {
        Self {
            safety: ConcurrencySafety::Safe,
        }
    }

    pub fn serialized() -> Self {
        Self {
            safety: ConcurrencySafety::Serialized,
        }
    }
}

impl Default for ConcurrencyDeclaration {
    fn default() -> Self {
        Self::serialized()
    }
}

/// 取消声明：只声明协作能力，不携带 timeout 策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CancellationDeclaration {
    /// timeout 后请求取消并等待受控清理。
    Cooperative,
    /// timeout 后可能继续副作用；必须提示风险并限制同名重入。
    NonCooperative,
}

// ── ToolDescriptor ──────────────────────────────────────────────────

/// Tool Catalog 的 Published Language。
///
/// 不包含 Tool 实例、来源 adapter、MCP server、函数指针、transport、client
/// 或 Registry 引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: ToolName,
    /// 工具描述（注入 LLM 的 tool schema 用）。
    pub description: String,
    /// 输入 JSON Schema。
    pub input_schema: serde_json::Value,
    /// 执行所需能力；Profile 必须全部覆盖才允许进入 Catalog。
    pub required_capabilities: ToolCapabilities,
    /// 并发声明。
    pub concurrency: ConcurrencyDeclaration,
    /// 取消声明。
    pub cancellation: CancellationDeclaration,
}

impl ToolDescriptor {
    /// 该 Descriptor 的并发安全是否为 Safe。
    pub fn is_concurrency_safe(&self) -> bool {
        self.concurrency.safety == ConcurrencySafety::Safe
    }

    /// 该 Descriptor 是否支持协作取消。
    pub fn is_cooperative_cancel(&self) -> bool {
        self.cancellation == CancellationDeclaration::Cooperative
    }
}

// ── ToolInvocation ──────────────────────────────────────────────────

/// 工具调用请求。
///
/// 不携带 RuntimeContext、Registry、Session、Store 或 MCP 类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub tool_name: ToolName,
    pub input: serde_json::Value,
}

impl ToolInvocation {
    pub fn new(tool_name: impl Into<ToolName>, input: serde_json::Value) -> Self {
        Self {
            tool_name: tool_name.into(),
            input,
        }
    }
}

// ── ToolErrorKind ───────────────────────────────────────────────────

/// 工具执行错误分类。
///
/// `ToolOutcome::Failure` 使用此分类，保证未知、越权、非法参数分类稳定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolErrorKind {
    /// 工具不存在或不在当前 Scope 内。
    ToolUnavailable,
    /// 参数不符合当前 schema。
    InvalidInput,
    /// Profile 不允许该 Tool 的全部 capabilities。
    Unauthorized,
    /// required resources 不可用。
    ResourceUnavailable,
    /// 内部执行错误（adapter / transport 等）。
    Internal,
}

// ── ToolOutcome ─────────────────────────────────────────────────────

/// 内容块（简化版，后续可扩展）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    pub text: String,
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// 工具执行元数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolExecutionMetadata {
    /// 执行耗时（毫秒）。
    pub duration_ms: Option<u64>,
}

/// 工具成功结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSuccess {
    pub content: Vec<ContentBlock>,
    /// 结构化数据（给 TUI / server 边界反序列化）。
    pub data: Option<serde_json::Value>,
    pub metadata: ToolExecutionMetadata,
}

impl ToolSuccess {
    /// 从文本创建最简成功结果。
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::text(text)],
            data: None,
            metadata: ToolExecutionMetadata::default(),
        }
    }
}

/// 工具失败结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFailure {
    pub kind: ToolErrorKind,
    /// 可安全暴露的错误消息（不泄漏密钥、协议私有信息）。
    pub safe_message: String,
    pub retryable: bool,
    pub content: Vec<ContentBlock>,
    pub data: Option<serde_json::Value>,
}

impl ToolFailure {
    pub fn new(kind: ToolErrorKind, safe_message: impl Into<String>) -> Self {
        let msg = safe_message.into();
        let retryable = matches!(
            kind,
            ToolErrorKind::Internal | ToolErrorKind::ResourceUnavailable
        );
        Self {
            kind,
            safe_message: msg.clone(),
            retryable,
            content: vec![ContentBlock::text(msg)],
            data: None,
        }
    }

    /// 便捷构造：ToolUnavailable。
    pub fn unavailable(name: &str) -> Self {
        Self::new(
            ToolErrorKind::ToolUnavailable,
            format!("工具「{name}」不存在或不在当前作用域内"),
        )
    }

    /// 便捷构造：InvalidInput。
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::InvalidInput, msg)
    }

    /// 便捷构造：Internal。
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Internal, msg)
    }
}

/// 工具取消结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCancelled {
    /// 取消原因描述。
    pub reason: String,
}

impl ToolCancelled {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

/// 工具执行结果（领域结果）。
///
/// 不依赖 SDK/TUI View。错误只公开可安全暴露的信息。
/// `ToolExecutionPort::execute` 使用单一 ToolOutcome 通道（含错误），
/// 避免调用方在 `Result::Err` 与 `ToolOutcome::Failure` 之间产生两套失败语义。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolOutcome {
    Success(ToolSuccess),
    Failure(ToolFailure),
    Cancelled(ToolCancelled),
}

impl ToolOutcome {
    pub fn success_text(text: impl Into<String>) -> Self {
        Self::Success(ToolSuccess::from_text(text))
    }

    pub fn failure(kind: ToolErrorKind, msg: impl Into<String>) -> Self {
        Self::Failure(ToolFailure::new(kind, msg))
    }

    pub fn cancelled(reason: impl Into<String>) -> Self {
        Self::Cancelled(ToolCancelled::new(reason))
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure(_))
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled(_))
    }
}

// ── RegistryScopeName / ToolProfileName ─────────────────────────────

/// Registry Scope 名称标识。
///
/// Scope 是一次 RuntimeContext 装配出的 Tool 实例与资源集合。
/// 例如：Main Scope、Sub Scope。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegistryScopeName(String);

impl RegistryScopeName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RegistryScopeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for RegistryScopeName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// Tool Profile 名称标识。
///
/// Profile 是能力允许集合，回答"已装配能力中允许用什么"。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolProfileName(String);

impl ToolProfileName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolProfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for ToolProfileName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

// ── ToolCatalogSnapshot ─────────────────────────────────────────────

/// Tool Catalog 只读投影。
///
/// 由 [`crate::contract::ports::ToolCatalogPort::snapshot`] 返回。
/// 消费者只看到统一 `ToolDescriptor`，不接触来源实现。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCatalogSnapshot {
    pub scope: RegistryScopeName,
    pub profile: ToolProfileName,
    pub tools: Vec<ToolDescriptor>,
}

impl ToolCatalogSnapshot {
    pub fn new(
        scope: impl Into<RegistryScopeName>,
        profile: impl Into<ToolProfileName>,
        tools: Vec<ToolDescriptor>,
    ) -> Self {
        Self {
            scope: scope.into(),
            profile: profile.into(),
            tools,
        }
    }

    /// 按 name 查找 Descriptor。
    pub fn find(&self, name: &ToolName) -> Option<&ToolDescriptor> {
        self.tools.iter().find(|d| d.name == *name)
    }

    /// Snapshot 中工具数量。
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

// ── Catalog 错误 ────────────────────────────────────────────────────

/// Catalog 投影错误。
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum ToolCatalogError {
    #[error("未知的 Registry Scope: {scope}")]
    UnknownScope { scope: String },

    #[error("未知的 Tool Profile: {profile}")]
    UnknownProfile { profile: String },

    #[error("Scope 装配错误: {reason}")]
    ScopeAssembly { reason: String },
}

// ══════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── ToolName ────────────────────────────────────────────────────

    #[test]
    fn test_tool_name_normalizes_to_lowercase() {
        let name = ToolName::new("Read");
        assert_eq!(name.normalized(), "read");
        assert_eq!(name.as_str(), "read");
    }

    #[test]
    fn test_tool_name_preserves_mcp_qualified_name() {
        let name = ToolName::new("mcp__Server__Tool");
        assert_eq!(name.normalized(), "mcp__server__tool");
    }

    #[test]
    fn test_tool_name_equality_is_case_insensitive() {
        let a = ToolName::new("Bash");
        let b = ToolName::new("BASH");
        assert_eq!(a, b);
    }

    #[test]
    fn test_tool_name_display() {
        let name = ToolName::new("Grep");
        assert_eq!(format!("{name}"), "grep");
    }

    // ── ToolCapabilities ───────────────────────────────────────────

    #[test]
    fn test_capabilities_contains_cap() {
        let caps = ToolCapabilities::ReadWorkspace | ToolCapabilities::WriteWorkspace;
        assert!(caps.contains_cap(ToolCapability::ReadWorkspace));
        assert!(caps.contains_cap(ToolCapability::WriteWorkspace));
        assert!(!caps.contains_cap(ToolCapability::ExecuteProcess));
    }

    #[test]
    fn test_capabilities_from_caps() {
        let caps = ToolCapabilities::from_caps([
            ToolCapability::ReadWorkspace,
            ToolCapability::NetworkAccess,
        ]);
        assert!(caps.contains_cap(ToolCapability::ReadWorkspace));
        assert!(caps.contains_cap(ToolCapability::NetworkAccess));
        assert!(!caps.contains_cap(ToolCapability::WriteWorkspace));
    }

    #[test]
    fn test_capabilities_is_subset_of() {
        let full = ToolCapabilities::all();
        let partial = ToolCapabilities::ReadWorkspace | ToolCapabilities::WriteWorkspace;
        assert!(partial.is_subset_of(full));
        assert!(!full.is_subset_of(partial));
    }

    #[test]
    fn test_capabilities_empty_is_subset_of_anything() {
        let empty = ToolCapabilities::empty();
        let some = ToolCapabilities::ReadWorkspace;
        assert!(empty.is_subset_of(some));
        assert!(empty.is_subset_of(ToolCapabilities::empty()));
    }

    // ── ConcurrencyDeclaration ─────────────────────────────────────

    #[test]
    fn test_concurrency_safe_construction() {
        let safe = ConcurrencyDeclaration::safe();
        assert_eq!(safe.safety, ConcurrencySafety::Safe);
        assert!(safe.safety == ConcurrencySafety::Safe);
    }

    #[test]
    fn test_concurrency_serialized_construction() {
        let serialized = ConcurrencyDeclaration::serialized();
        assert_eq!(serialized.safety, ConcurrencySafety::Serialized);
    }

    #[test]
    fn test_concurrency_default_is_serialized() {
        assert_eq!(
            ConcurrencyDeclaration::default().safety,
            ConcurrencySafety::Serialized
        );
    }

    // ── CancellationDeclaration ────────────────────────────────────

    #[test]
    fn test_cancellation_variants() {
        assert_ne!(
            CancellationDeclaration::Cooperative,
            CancellationDeclaration::NonCooperative
        );
    }

    // ── ToolDescriptor ─────────────────────────────────────────────

    #[test]
    fn test_descriptor_is_concurrency_safe() {
        let desc = ToolDescriptor {
            name: ToolName::new("Glob"),
            description: "File glob tool".into(),
            input_schema: serde_json::json!({"type": "object"}),
            required_capabilities: ToolCapabilities::ReadWorkspace,
            concurrency: ConcurrencyDeclaration::safe(),
            cancellation: CancellationDeclaration::Cooperative,
        };
        assert!(desc.is_concurrency_safe());
        assert!(desc.is_cooperative_cancel());
    }

    #[test]
    fn test_descriptor_serialized_and_non_cooperative() {
        let desc = ToolDescriptor {
            name: ToolName::new("Bash"),
            description: "Shell tool".into(),
            input_schema: serde_json::json!({"type": "object"}),
            required_capabilities: ToolCapabilities::ExecuteProcess
                | ToolCapabilities::WriteWorkspace,
            concurrency: ConcurrencyDeclaration::serialized(),
            cancellation: CancellationDeclaration::NonCooperative,
        };
        assert!(!desc.is_concurrency_safe());
        assert!(!desc.is_cooperative_cancel());
    }

    // ── ToolInvocation ─────────────────────────────────────────────

    #[test]
    fn test_tool_invocation_construction() {
        let inv = ToolInvocation::new("Read", serde_json::json!({"path": "/tmp"}));
        assert_eq!(inv.tool_name, ToolName::new("read"));
        assert_eq!(inv.input["path"], "/tmp");
    }

    // ── ToolErrorKind ──────────────────────────────────────────────

    #[test]
    fn test_tool_error_kind_equality() {
        assert_eq!(
            ToolErrorKind::ToolUnavailable,
            ToolErrorKind::ToolUnavailable
        );
        assert_ne!(ToolErrorKind::InvalidInput, ToolErrorKind::Internal);
    }

    // ── ToolOutcome ────────────────────────────────────────────────

    #[test]
    fn test_outcome_success_text() {
        let o = ToolOutcome::success_text("done");
        assert!(o.is_success());
        assert!(!o.is_failure());
        assert!(!o.is_cancelled());
        match o {
            ToolOutcome::Success(s) => {
                assert_eq!(s.content.len(), 1);
                assert_eq!(s.content[0].text, "done");
            }
            _ => panic!("应为 Success"),
        }
    }

    #[test]
    fn test_outcome_failure_unavailable() {
        let o = ToolOutcome::failure(ToolErrorKind::ToolUnavailable, "not found");
        assert!(o.is_failure());
        match o {
            ToolOutcome::Failure(f) => {
                assert_eq!(f.kind, ToolErrorKind::ToolUnavailable);
                assert!(!f.retryable);
            }
            _ => panic!("应为 Failure"),
        }
    }

    #[test]
    fn test_outcome_failure_internal_is_retryable() {
        let o = ToolOutcome::failure(ToolErrorKind::Internal, "oops");
        assert!(o.is_failure());
        match o {
            ToolOutcome::Failure(f) => assert!(f.retryable),
            _ => panic!("应为 Failure"),
        }
    }

    #[test]
    fn test_outcome_cancelled() {
        let o = ToolOutcome::cancelled("user cancelled");
        assert!(o.is_cancelled());
        match o {
            ToolOutcome::Cancelled(c) => assert_eq!(c.reason, "user cancelled"),
            _ => panic!("应为 Cancelled"),
        }
    }

    #[test]
    fn test_tool_failure_unavailable_helper() {
        let f = ToolFailure::unavailable("Agent");
        assert_eq!(f.kind, ToolErrorKind::ToolUnavailable);
        assert!(f.safe_message.contains("Agent"));
        assert!(!f.retryable);
    }

    #[test]
    fn test_tool_failure_invalid_input_helper() {
        let f = ToolFailure::invalid_input("missing field: path");
        assert_eq!(f.kind, ToolErrorKind::InvalidInput);
        assert!(!f.retryable);
    }

    #[test]
    fn test_tool_failure_retryable_classification() {
        assert!(!ToolFailure::new(ToolErrorKind::ToolUnavailable, "").retryable);
        assert!(!ToolFailure::new(ToolErrorKind::InvalidInput, "").retryable);
        assert!(!ToolFailure::new(ToolErrorKind::Unauthorized, "").retryable);
        assert!(ToolFailure::new(ToolErrorKind::ResourceUnavailable, "").retryable);
        assert!(ToolFailure::new(ToolErrorKind::Internal, "").retryable);
    }

    // ── RegistryScopeName / ToolProfileName ────────────────────────

    #[test]
    fn test_registry_scope_name_display() {
        let s = RegistryScopeName::new("main");
        assert_eq!(s.as_str(), "main");
        assert_eq!(format!("{s}"), "main");
    }

    #[test]
    fn test_tool_profile_name_display() {
        let p = ToolProfileName::new("full");
        assert_eq!(p.as_str(), "full");
        assert_eq!(format!("{p}"), "full");
    }

    // ── ToolCatalogSnapshot ────────────────────────────────────────

    #[test]
    fn test_catalog_snapshot_find() {
        let desc1 = ToolDescriptor {
            name: ToolName::new("Read"),
            description: "Read tool".into(),
            input_schema: serde_json::json!({"type": "object"}),
            required_capabilities: ToolCapabilities::ReadWorkspace,
            concurrency: ConcurrencyDeclaration::safe(),
            cancellation: CancellationDeclaration::Cooperative,
        };
        let desc2 = ToolDescriptor {
            name: ToolName::new("Bash"),
            description: "Bash tool".into(),
            input_schema: serde_json::json!({"type": "object"}),
            required_capabilities: ToolCapabilities::ExecuteProcess,
            concurrency: ConcurrencyDeclaration::serialized(),
            cancellation: CancellationDeclaration::NonCooperative,
        };
        let snapshot = ToolCatalogSnapshot::new("main", "full", vec![desc1, desc2]);

        assert_eq!(snapshot.len(), 2);
        assert!(!snapshot.is_empty());
        assert!(snapshot.find(&ToolName::new("read")).is_some());
        assert!(snapshot.find(&ToolName::new("READ")).is_some());
        assert!(snapshot.find(&ToolName::new("grep")).is_none());
    }

    #[test]
    fn test_catalog_snapshot_empty() {
        let snapshot = ToolCatalogSnapshot::new("sub", "restricted", vec![]);
        assert!(snapshot.is_empty());
        assert_eq!(snapshot.len(), 0);
    }

    // ── ToolCatalogError ───────────────────────────────────────────

    #[test]
    fn test_catalog_error_display() {
        let e = ToolCatalogError::UnknownScope {
            scope: "xyz".into(),
        };
        assert!(e.to_string().contains("xyz"));
    }
}
