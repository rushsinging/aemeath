//! Config-owned 出站端口契约。
//!
//! ## 设计目标
//!
//! - Config domain 定义满足用例所需的窄出站端口，**不**在 domain 内调用平台 API、
//!   fs 或网络；
//! - Config platform adapter 实现 [`SystemInformationPort`]；Provider Probe 端口
//!   由 Composition 注入 Provider adapter 实现；
//! - Connect bootstrap 由后续 Task 单独定义 `GlobalConfigConnectStore` 与首次聊天
//!   初始化；本 Task **不**引入新的 Commit seamble；
//! - 各 trait 的输入是已校验、已解析的值对象，**NEVER** 接收未验证字符串或
//!   内部类型（`ConfigSnapshot`、`&Path` 等）。
//!
//! ## 与 Connect 状态机的关系
//!
//! Provider 探测（[`ProviderProbePort`]）由 Config application 拥有的 Connect
//! 服务调用。Probe 输入为已校验的 driver / endpoint / credential / 模型 /
//! `max_tokens` / 最终 UA / timeout，不接收 TUI 类型、不暴露内部状态。

use std::time::Duration;

use crate::catalog::DriverId;

/// Config platform adapter 必须提供的最小系统信息。
///
/// `os_version` 在不可用或为空时保持 `None`，UA resolver 会降级为
/// `Aemeath/<version> cli <os>/<arch>` 形式（参见 [`crate::user_agent`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInformation {
    pub os_name: String,
    pub os_version: Option<String>,
    pub arch: String,
}

/// Config-owned 平台信息端口。
///
/// ## 设计约束
///
/// - `async_trait` 保持与其他 Config 端口一致；
/// - 实现方必须**只**返回受控、非敏感字段；不得把 hostname、user、cwd、API key
///   等敏感信息塞入 [`SystemInformation`]；
/// - 实现方负责内部缓存与降级逻辑；调用方信任 `os_name` / `os_version` / `arch`
///   是干净字符串，但 UA resolver 仍会做一次 HeaderValue 校验。
#[async_trait::async_trait]
pub trait SystemInformationPort: Send + Sync {
    async fn current(&self) -> SystemInformation;
}

// ---------------------------------------------------------------------------
// Provider 探测
// ---------------------------------------------------------------------------

/// Provider 探测的归一化请求。
///
/// 由 Connect application 在调用 Provider adapter 之前构造：
/// - `driver` / `base_url` / `model_id` / `context_window` / `max_tokens`
///   来自 Connect 服务归一化后的 draft；
/// - `credential` 是 Connect 服务**内存**内的字符串；调用方在调用前必须清空
///   任何路径或日志上下文中的明文凭证；
/// - `final_user_agent` 是 Config-owned UA resolver 解析后的字符串；
///   Provider adapter 不应再次 fallback 或重新解析；
/// - `timeout` 由 Connect 服务根据全局策略注入，避免不同入口漂移。
///
/// 该对象**仅包含**调用所需的归一化字段；**NEVER** 把 `ConnectSessionId`、
/// 任何路径或 `ConfigSnapshot` 等内部结构塞入本请求。
#[derive(Debug, Clone)]
pub struct ProviderProbeRequest {
    pub driver: DriverId,
    pub base_url: String,
    pub credential: Option<String>,
    pub model_id: String,
    pub context_window: usize,
    pub max_tokens: u32,
    pub final_user_agent: String,
    pub timeout: Duration,
}

/// Provider 探测结果。`latency` 仅用于可观测性，不参与业务判定。
#[derive(Debug, Clone)]
pub struct ProviderProbeResult {
    pub latency: Duration,
}

/// Provider 探测的稳定错误分类。
///
/// 与 [`crate::connect::ConnectError::ProbeFailed`] 协同映射：
/// - Connect 服务**不**在自身包装一层 `ProbeFailed` 之外的细节；
/// - Adapter 错误（HTTP body / wire DTO）必须在 Config ↔ Provider 边界
///   映射为上述稳定类别，禁止泄漏敏感正文。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderProbeErrorKind {
    /// 用户在向导中显式取消探测。
    Cancelled,
    /// 探测超过 Connect 服务注入的 timeout。
    Timeout,
    /// 服务端拒绝凭证（HTTP 401/403 等）。
    Authentication,
    /// endpoint 不可达或格式错误（DNS / TCP / 状态码语义不匹配）。
    Endpoint,
    /// 模型 id 在该 endpoint 上不可用或响应语义错误。
    Model,
    /// 响应语义违反 provider 协议（非 JSON / 流式不匹配等）。
    Protocol,
    /// 未分类内部错误。Adapter 必须将正文降为红色摘要，避免泄漏 API Key。
    Internal,
}

/// Provider 探测 typed error。`message` 是脱敏后的可展示描述。
#[derive(Debug, Clone)]
pub struct ProviderProbeError {
    pub kind: ProviderProbeErrorKind,
    pub message: String,
}

impl std::fmt::Display for ProviderProbeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProviderProbeError {}

/// Config-owned Provider 探测端口。
///
/// ## 实现约束
///
/// - 发送供应商协议允许的最小 LLM 请求，**禁止**仅做 TCP / HTTP HEAD /
///   模型列表冒充成功；
/// - 与正式请求共用同一 Provider request ACL、认证 Header 规则与 UA 解析
///   结果；Provider adapter **NEVER** 在 Probe 路径内重新构造 UA 或解析
///   Catalog；
/// - 单次调用、不透明重试；取消 / 超时 / 认证 / endpoint / model / 协议
///   错误必须映射为 [`ProviderProbeErrorKind`] 中的稳定类别；
/// - **NEVER** 写入 committed config；**NEVER** 读取 env；
/// - 错误与日志必须清洗 API Key、Authorization Header 与敏感响应正文。
#[async_trait::async_trait]
pub trait ProviderProbePort: Send + Sync {
    async fn probe(
        &self,
        request: ProviderProbeRequest,
    ) -> Result<ProviderProbeResult, ProviderProbeError>;
}
