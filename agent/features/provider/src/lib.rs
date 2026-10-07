//! Provider：LLM 上游的 HTTP/stream 实现与请求编排。
//!
//! # Published Language（#1861 wire 收敛后形态）
//!
//! | 组（DDD 五类） | 实体与判定 |
//! |---|---|
//! | 工厂（wire，经 `composition` 构造面） | `wire_provider_client`（客户端装配：from_config_with_pool + 默认推理档位）、`wire_provider_assembly`（factory build 内核：客户端 + capability + 生效档位）、`wire_probe_client` + `run_connectivity_probe`（connect 探测构造与执行语义）。组合根 NEVER 直调 `LlmClient` 构造器（construction_symbols 登记） |
//! | 端口 | `LlmProvider`（上游驱动 trait；签名载荷 `InvocationScopeData`/`SystemBlockData` 随导出豁免，runtime 测试 fake 消费）、`CancellationSignal`（**归 #1739 信号统一，冻结**） |
//! | 值对象/命令/事件 | `InvocationRequestData`（命令，含 options 组合——runtime 生产真消费，非冗余）、`InvocationOptionsData`、`InvocationStreamData`、`InvocationEventData`/`InvocationDeltaData`（事件）、`ModelIdData`、`ModelCapabilityData`（supports_* 真值化待 ReasoningMappingKindData 处置）、`ModelToolSchemaData`、`RequestSystemBlockData`、`Provider{Completion,ContentBlock,ToolCall,ToolCallId,StopReason}Data`（Completion/ToolCallId 为载荷豁免）、`RawUsageSnapshotData`、`ReasoningCapabilityData`、`ReasoningMappingKindData`（写后不读，处置待裁决）、`ProviderDriverKind`（= share DriverKind 身份词表，#1850 敏感区单一真相源） |
//! | 实现体（构造面豁免） | `LlmClient`、`TransportPool`、`LlmConfigOptionsData`、`ProviderAssemblyWiring`——仅组合根桥接/翻译所需，业务消费者不得引用（composition 模块标注） |
//! | Error | `ProviderError`（retryable/retry_after 迁出归 **#1740**）、`ProviderErrorKind`（13 变体）、`LlmError`（构造方法签名载荷豁免：from_config 族返回 Result<_, LlmError>；错误面折叠归 #1740）——Error 类**不 Data 化** |
//!
//! #1861 收敛记录：删 legacy `new`/`with_provider`/`LlmProviderOptions`（#1850
//! UA 静默回落形态）与 `ReasoningLevel` 三跳转发；StopReason 双定义统一 PL 版
//! （6 变体）；HTTP/SSE wire DTO 下移 `adapters/wire.rs`（domain 只留
//! InvocationScopeData）；driver 词表唯一真相源 share::config::domain::driver_kind。
//! facade 白名单 28 符号与根导出面 exact-match（dump 法核对）。
//! 按 docs/design/03-engineering/05-published-language.md SOP。

#![deny(clippy::print_stdout, clippy::print_stderr)]

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod constants;
pub(crate) use constants::{
    ANTHROPIC_STREAM_IDLE_TIMEOUT_SECS, CONNECT_TIMEOUT_SECS, LOG_TARGET,
    OLLAMA_STREAM_IDLE_TIMEOUT_SECS, OPENAI_STREAM_IDLE_TIMEOUT_SECS, STALL_THRESHOLD_SECS,
};

mod adapters;
mod domain;
mod ports;
pub mod published_language;

pub use domain::capability::ProviderDriverKind;
pub(crate) use domain::invoke::InvocationScopeData;

/// Composition Root 专用构造面；业务消费者不得引用。
pub mod composition {
    pub use crate::adapters::client::{
        wire_provider_assembly, wire_provider_client, LlmClient, LlmConfigOptionsData,
        ProviderAssemblyWiring,
    };
    pub use crate::adapters::pool::TransportPool;
    pub use crate::adapters::probe::{run_connectivity_probe, wire_probe_client};
    pub use crate::adapters::wire::SystemBlockData;
    pub use crate::domain::capability::reasoning_capability_from_max;
    pub use crate::domain::invoke::InvocationScopeData;
    pub use crate::ports::LlmProvider;
}

pub use published_language::{
    CancellationSignal, InvocationDeltaData, InvocationEventData, InvocationOptionsData,
    InvocationRequestData, InvocationStreamData, ModelCapabilityData, ModelIdData,
    ModelToolSchemaData, ProviderCompletionData, ProviderContentBlockData, ProviderError,
    ProviderErrorKind, ProviderStopReasonData, ProviderToolCallData, ProviderToolCallIdData,
    RawUsageSnapshotData, ReasoningCapabilityData, ReasoningMappingKindData,
    RequestSystemBlockData,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum LlmError {
    #[error("API error [{error_type}]: {message}")]
    Api { error_type: String, message: String },
    #[error("request cancelled by user")]
    Cancelled,
    #[error("stream error: {0}")]
    Stream(String),
    #[error("stream connection interrupted: {0}")]
    StreamInterrupted(String),
    #[error("config error: {0}")]
    Config(String),
    #[error(
        "stream truncated mid-tool_call '{tool_call_name}' (id={tool_call_id}): {accumulated_bytes} bytes across {delta_count} deltas — provider closed SSE early"
    )]
    StreamTruncated {
        tool_call_id: String,
        tool_call_name: String,
        accumulated_bytes: usize,
        delta_count: u32,
        head_preview: String,
        tail_preview: String,
    },
}

// ─── LlmError → ProviderError 权威映射（crate 内三 driver + stream + runtime 装配共用）───

impl From<LlmError> for ProviderError {
    fn from(error: LlmError) -> Self {
        let kind = match &error {
            LlmError::Cancelled => ProviderErrorKind::Cancelled,
            LlmError::Api { .. } => ProviderErrorKind::UpstreamUnavailable,
            LlmError::StreamInterrupted(_) | LlmError::StreamTruncated { .. } => {
                ProviderErrorKind::StreamTruncated
            }
            LlmError::Stream(_) => ProviderErrorKind::Protocol,
            LlmError::Config(_) => ProviderErrorKind::Configuration,
        };
        ProviderError::fatal(kind, error.to_string())
    }
}
#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
