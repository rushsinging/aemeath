//! Provider：LLM 上游的防腐网关服务（request → response）。
//!
//! # Published Language（#1861 v4 收敛，#1880 review 拍板）
//!
//! provider 无自有聚合（invocation 是行为、model 是外部数据）——PL 只
//! 发布端口函数签名：一类输入 + 一族输出 + 工厂 + 错误。
//!
//! | 组 | 实体与判定 |
//! |---|---|
//! | **输入（1）** | `ProviderRequestData`（model + max_output_tokens + reasoning + 整段 system prompt + static_prefix_len 缓存前缀分界 + messages + wire-ready tools + cancellation token 全内聚；判定：不更名 ProviderRequestData——现名已表意，更名无行为收益。Options/Scope 等中间形态已消失；#1861 v4 块级 system 块类型消除，拼接归 context/runtime） |
//! | **输出（两类）** | 非流式 `ProviderResponse`（自带 ok/error，单通道）+ 流式 `ProviderResponseChunk`（Content/Usage/Stop/Error 帧，Error 即失败终止帧）→ `ProviderResponseStream`；载荷 `ProviderContentData`（终态块+流增量合并家族；tool call 以 `ToolCall{id,name,arguments}` / `ToolCallCompleted{index,id,name,arguments}` 命名字段内联——#1861 v4 tool call 独立载荷类型消除）/`ResponseStopReason`/`TokenUsageData`（LLM token 计量；audit 快照由 audit 域组装） |
//! | 模型元数据 | `ModelInfo`（单实体：身份 + 能力，config/catalog 外部数据在 provider 读侧的完整投影 + `supported_reasoning` 阶梯与 `resolve_reasoning` clamp 方法——#1861 v4 reasoning 能力数据类型摊平进实体；#1880 裁决：model 信息只有一个实体来源，provider 不负责写入） |
//! | 工厂（2 入口） | `wire_provider_client`（主链路：config + 模型元数据 → `(client, 修正版 ModelInfo)`，阶梯由 client 推导覆盖）、`wire_test_provider_client`（connect 探测构造+执行合一） |
//! | 构造面豁免 | `LlmClient`/`ProviderClientSpecData`/`TransportPool`——仅组合根桥接所需 |
//! | Error | `ProviderError` + `ProviderErrorKind`（13 变体；LlmError 已内部化，折叠归 #1740） |
//! | 身份 | `ProviderDriverKind`（= share DriverKind 词表，#1850 单一真相源） |
//!
//! #1861 v2 收敛记录：C10 LlmError 内部化；C11 ToolSchemaData 归 context
//! （request.tools wire-ready Value）；C12 request 单载体 + trait 中性化
//! （ResolvedInvocation/scope 内部化、中性 system 块、wire DTO 下移
//! adapters）；C12c runtime fake 自产事件流、构造面撤空；C13
//! CancellationSignal 归 runtime（取消单通道 request.cancellation）；
//! C14 工厂收敛 2 入口。
//! #1861 v4 收敛记录（用户逐项拍板 7 项）：客户端构造配置类型更名
//! ProviderClientSpecData；工厂收敛为 wire_provider_client（装配句柄包
//! 消除）；tool call 载荷摊平进 ProviderContentData 命名字段；reasoning
//! 能力数据摊平进 ModelInfo.supported_reasoning + resolve_reasoning；
//! system 块类型消除（整串 + static_prefix_len）；探测入口更名
//! wire_test_provider_client；白名单随行更新。
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
    ModelInfo, ProviderContentData, ProviderError, ProviderErrorKind, ProviderRequestData,
    ProviderResponse, ProviderResponseChunk, ProviderResponseStream, ResponseStopReason,
    TokenUsageData,
};

pub(crate) use domain::capability::ProviderDriverKind;

/// Composition Root 专用构造面；业务消费者不得引用。
pub mod composition {
    pub use crate::adapters::client::{wire_provider_client, LlmClient, ProviderClientSpecData};
    pub use crate::adapters::pool::TransportPool;
    pub use crate::adapters::probe::wire_test_provider_client;
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
