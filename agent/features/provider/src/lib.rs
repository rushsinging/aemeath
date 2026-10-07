//! Provider：LLM 上游的防腐网关服务（request → response）。
//!
//! # Published Language（#1861 v2 终态，#1880 review 拍板）
//!
//! provider 无自有聚合（invocation 是行为、model 是外部数据）——PL 只
//! 发布端口函数签名：一类输入 + 一族输出 + 工厂 + 错误。
//!
//! | 组 | 实体与判定 |
//! |---|---|
//! | **输入（1）** | `InvocationRequestData`（model + max_output_tokens + reasoning + system 中性块 + messages + wire-ready tools + cancellation token 全内聚；判定：不更名 ProviderRequestData——现名已表意，更名无行为收益。Options/Scope 等中间形态已消失） |
//! | **输出（两类）** | 非流式 `ProviderResponse`（自带 ok/error，单通道）+ 流式 `ProviderResponseChunk`（Content/Usage/Stop/Error 帧，Error 即失败终止帧）→ `ProviderResponseStream`；载荷 `ProviderContentData`（终态块+流增量合并家族）/`ProviderToolCallData`/`ProviderStopReasonData`/`TokenUsageData`（LLM token 计量；audit 快照由 audit 域组装）
//! | 模型元数据 | `ModelInfo`（单实体：身份 + 能力，config/catalog 外部数据在 provider 读侧的完整投影——#1880 裁决：model 信息只有一个实体来源，provider 不负责写入） |
//! | 工厂（2 入口） | `wire_provider_assembly`（主链路：client + ModelInfo + resolved 档位，返回 `ProviderAssemblyWiring` 构造面句柄包）、`probe_connectivity`（connect 探测构造+执行合一） |
//! | 构造面豁免 | `LlmClient`/`LlmConfigOptionsData`/`TransportPool`——仅组合根桥接所需 |
//! | Error | `ProviderError` + `ProviderErrorKind`（13 变体；LlmError 已内部化，折叠归 #1740） |
//! | 身份 | `ProviderDriverKind`（= share DriverKind 词表，#1850 单一真相源） |
//!
//! #1861 v2 收敛记录：C10 LlmError 内部化；C11 ToolSchemaData 归 context
//! （request.tools wire-ready Value）；C12 request 单载体 + trait 中性化
//! （ResolvedInvocation/scope 内部化、中性 system 块、wire DTO 下移
//! adapters）；C12c runtime fake 自产事件流、构造面撤空；C13
//! CancellationSignal 归 runtime（取消单通道 request.cancellation）；
//! C14 工厂收敛 2 入口。白名单 25 项 exact-match（dump 核对）。
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

pub use published_language::{
    InvocationRequestData, ModelInfo, ProviderContentData, ProviderError, ProviderErrorKind,
    ProviderResponse, ProviderResponseChunk, ProviderResponseStream, ProviderStopReasonData,
    ProviderToolCallData, ReasoningCapabilityData, RequestSystemBlockData, TokenUsageData,
};

pub(crate) use domain::capability::ProviderDriverKind;

/// Composition Root 专用构造面；业务消费者不得引用。
pub mod composition {
    pub use crate::adapters::client::{
        wire_provider_assembly, LlmClient, LlmConfigOptionsData, ProviderAssemblyWiring,
    };
    pub use crate::adapters::pool::TransportPool;
    pub use crate::adapters::probe::probe_connectivity;
}

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
