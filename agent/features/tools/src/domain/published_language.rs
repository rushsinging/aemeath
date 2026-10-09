//! Tool 领域 Published Language（DDD §6.4.3）。
//!
//! 本模块定义 Tool BC 的值对象和领域类型，供 [`crate::domain::ports`]
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

use super::task_change::CommittedTaskChange;
use super::{ExecutionScope, ToolSuspension};
use serde::{Deserialize, Serialize};
use std::fmt;

// 跨 BC 共享词汇已下沉 share（policy/runtime 的 domain 层依赖 share 而非本 crate）。
// 此处 re-export 保持 `crate::domain::*` 与 crate 根路径兼容。
pub use share::tools_vocab::{ToolCapabilities, ToolCapability, ToolName};

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

/// Runtime-safe declaration for input-dependent auto-approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputSafetyDeclaration {
    Always,
    Never,
    ReadOnlyShellCommand,
}

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
    /// Runtime-owned timeout policy reads this value; dispatch does not enforce it.
    pub timeout_secs: u64,
    /// Value-only approval declarations (no executable Tool callback escapes Catalog).
    pub read_only: bool,
    pub input_safety: InputSafetyDeclaration,
    /// Output JSON Schema used by presentation layers.
    pub data_schema: serde_json::Value,
    /// 输出直绑声明（#1890）：工具自声明可直绑子进程输出到任务日志文件；
    /// 未声明（false）工具行为完全不变。serde default 兼容旧快照。
    #[serde(default)]
    pub background_log_direct: bool,
}

impl ToolDescriptor {
    /// 该 Descriptor 的并发安全是否为 Safe。
    pub fn is_concurrency_safe(&self) -> bool {
        self.concurrency.safety == ConcurrencySafety::Safe
    }

    /// 输出直绑能力（#1890）：runtime 据此决定是否为该工具的调用
    /// 创建任务日志文件并注入路径。
    pub fn is_background_log_direct(&self) -> bool {
        self.background_log_direct
    }

    /// 该 Descriptor 是否支持协作取消。
    pub fn is_cooperative_cancel(&self) -> bool {
        self.cancellation == CancellationDeclaration::Cooperative
    }

    pub fn is_input_safe(&self, input: &serde_json::Value) -> bool {
        match self.input_safety {
            InputSafetyDeclaration::Always => true,
            InputSafetyDeclaration::Never => false,
            InputSafetyDeclaration::ReadOnlyShellCommand => input
                .get("command")
                .and_then(serde_json::Value::as_str)
                .is_some_and(crate::domain::shell_safety::is_readonly_command),
        }
    }
}

// ── ToolInvocation ──────────────────────────────────────────────────

/// 工具调用请求。
///
/// 这是可序列化语义上的纯值描述：不携带 RuntimeContext、Registry、Session、
/// Store、MCP client、token、channel 或 callback capability。
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    pub tool_name: ToolName,
    pub input: serde_json::Value,
    pub execution_scope: ExecutionScope,
    pub authorization: crate::domain::AuthorizationContext,
}

impl ToolInvocation {
    pub fn new(
        tool_name: impl Into<ToolName>,
        input: serde_json::Value,
        execution_scope: ExecutionScope,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            input,
            execution_scope,
            authorization: crate::domain::AuthorizationContext::STANDARD,
        }
    }

    pub fn with_authorization(
        mut self,
        authorization: crate::domain::AuthorizationContext,
    ) -> Self {
        self.authorization = authorization;
        self
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
    #[serde(skip)]
    pub task_change: Option<CommittedTaskChange>,
}

impl ToolSuccess {
    /// 从文本创建最简成功结果。
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::text(text)],
            data: None,
            metadata: ToolExecutionMetadata::default(),
            task_change: None,
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

/// 底层清理确认状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CleanupConfirmation {
    Confirmed,
    Unconfirmed,
    NotApplicable,
}

/// timeout / cancellation-unconfirmed 的安全终态详情。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolTerminalDetails {
    pub safe_reason: String,
    pub possible_side_effects: Vec<String>,
    pub unfinished_call_ids: Vec<String>,
    pub cleanup: CleanupConfirmation,
}

impl ToolTerminalDetails {
    pub fn new(safe_reason: impl Into<String>, cleanup: CleanupConfirmation) -> Self {
        Self {
            safe_reason: safe_reason.into(),
            possible_side_effects: Vec::new(),
            unfinished_call_ids: Vec::new(),
            cleanup,
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
    TimedOut(ToolTerminalDetails),
    CancellationUnconfirmed(ToolTerminalDetails),
    Suspended(ToolSuspension),
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

    pub fn timed_out(safe_reason: impl Into<String>, cleanup: CleanupConfirmation) -> Self {
        Self::TimedOut(ToolTerminalDetails::new(safe_reason, cleanup))
    }

    pub fn cancellation_unconfirmed(
        safe_reason: impl Into<String>,
        possible_side_effects: Vec<String>,
        unfinished_call_ids: Vec<String>,
    ) -> Self {
        Self::CancellationUnconfirmed(ToolTerminalDetails {
            safe_reason: safe_reason.into(),
            possible_side_effects,
            unfinished_call_ids,
            cleanup: CleanupConfirmation::Unconfirmed,
        })
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
/// 由 [`crate::domain::ports::ToolCatalogPort::snapshot`] 返回。
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

    pub fn model_schemas(&self) -> Vec<serde_json::Value> {
        self.tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name.as_str(),
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                    "data_schema": tool.data_schema,
                })
            })
            .collect()
    }

    pub fn selected(mut self, selection: &share::config::ToolSelection) -> Self {
        self.tools
            .retain(|descriptor| selection.allows(descriptor.name.as_str()));
        self
    }

    /// Snapshot 中工具数量。
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl crate::domain::ToolListProvider for ToolCatalogSnapshot {
    fn tool_names(&self) -> Vec<String> {
        self.tools
            .iter()
            .map(|descriptor| descriptor.name.as_str().to_string())
            .collect()
    }

    fn tool_description(&self, name: &str) -> Option<String> {
        self.find(&ToolName::new(name))
            .map(|descriptor| descriptor.description.clone())
    }

    fn tool_info(&self, name: &str) -> Option<crate::domain::types::tool_search::ToolInfo> {
        self.find(&ToolName::new(name)).map(|descriptor| {
            crate::domain::types::tool_search::ToolInfo {
                name: descriptor.name.as_str().to_string(),
                description: descriptor.description.clone(),
                input_schema: descriptor.input_schema.clone(),
                is_read_only: descriptor.read_only,
            }
        })
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
