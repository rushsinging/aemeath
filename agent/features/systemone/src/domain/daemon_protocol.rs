//! Scoring daemon UDS 协议：length-prefixed JSON 帧 + answer 请求/响应。
//!
//! 设计依据：`docs/design/02-modules/systemone/04-scoring-daemon.md` §5。

use serde::{Deserialize, Serialize};

use crate::constants::DAEMON_PROTOCOL_VERSION;
use crate::domain::published_language::{ScoringAnswer, ScoringQuestion, ScoringState};

/// 帧解码不完整或非法。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    Incomplete,
}

/// 协议层错误（版本 / method / JSON）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    Frame(FrameError),
    Json(String),
    VersionMismatch { found: u32, expected: u32 },
    UnsupportedMethod(String),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frame(FrameError::Incomplete) => formatter.write_str("协议帧不完整"),
            Self::Json(detail) => write!(formatter, "协议 JSON 非法：{detail}"),
            Self::VersionMismatch { found, expected } => write!(
                formatter,
                "协议版本不匹配：found={found} expected={expected}"
            ),
            Self::UnsupportedMethod(method) => {
                write!(formatter, "不支持的 method：{method}")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<FrameError> for ProtocolError {
    fn from(value: FrameError) -> Self {
        Self::Frame(value)
    }
}

/// `answer` 方法参数：对齐 `ScoringPort::answer` 输入。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<String>,
    pub state: ScoringState,
    pub questions: Vec<ScoringQuestion>,
}

/// 成功响应结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerResult {
    pub answers: Vec<ScoringAnswer>,
    pub engine_revision: String,
    pub latency_ms: u64,
}

/// 错误 kind：与设计文档 wire 字符串对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonErrorKind {
    Unavailable,
    Timeout,
    Protocol,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonErrorBody {
    pub kind: DaemonErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum DaemonRequest {
    Answer {
        protocol_version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        params: AnswerParams,
    },
}

#[cfg(test)]
impl DaemonRequest {
    /// 构造 answer 请求。
    pub fn answer(id: Option<String>, params: AnswerParams) -> Self {
        Self::Answer {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            id,
            params,
        }
    }

    pub fn protocol_version(&self) -> u32 {
        match self {
            Self::Answer {
                protocol_version, ..
            } => *protocol_version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DaemonResponse {
    Ok {
        protocol_version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        result: AnswerResult,
    },
    Err {
        protocol_version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        error: DaemonErrorBody,
    },
}

impl DaemonResponse {
    pub fn ok(id: Option<String>, result: AnswerResult) -> Self {
        Self::Ok {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            id,
            result,
        }
    }

    pub fn error(id: Option<String>, error: DaemonErrorBody) -> Self {
        Self::Err {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            id,
            error,
        }
    }
}

/// 编码 length-prefixed 帧：`u32` LE 长度 + body。
pub fn encode_frame(body: &[u8]) -> Vec<u8> {
    let length = u32::try_from(body.len()).expect("frame body 超过 u32::MAX");
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(body);
    frame
}

/// 尝试从 buffer 前缀解码一帧；成功返回 `(body, consumed_bytes)`。
pub fn try_decode_frame(buffer: &[u8]) -> Result<(&[u8], usize), FrameError> {
    if buffer.len() < 4 {
        return Err(FrameError::Incomplete);
    }
    let length = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    let total = 4 + length;
    if buffer.len() < total {
        return Err(FrameError::Incomplete);
    }
    Ok((&buffer[4..total], total))
}

pub fn encode_json_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, ProtocolError> {
    let body = serde_json::to_vec(value).map_err(|error| ProtocolError::Json(error.to_string()))?;
    Ok(encode_frame(&body))
}

pub fn decode_answer_request(buffer: &[u8]) -> Result<(DaemonRequest, usize), ProtocolError> {
    let (body, consumed) = try_decode_frame(buffer)?;
    let wire: AnswerRequestWire =
        serde_json::from_slice(body).map_err(|error| ProtocolError::Json(error.to_string()))?;
    if wire.protocol_version != DAEMON_PROTOCOL_VERSION {
        return Err(ProtocolError::VersionMismatch {
            found: wire.protocol_version,
            expected: DAEMON_PROTOCOL_VERSION,
        });
    }
    if wire.method != "answer" {
        return Err(ProtocolError::UnsupportedMethod(wire.method));
    }
    Ok((
        DaemonRequest::Answer {
            protocol_version: wire.protocol_version,
            id: wire.id,
            params: wire.params,
        },
        consumed,
    ))
}

/// 解码响应帧（客户端 / 测试用；生产客户端在 PR2 落地后去掉 cfg）。
#[cfg(test)]
pub fn decode_response(buffer: &[u8]) -> Result<(DaemonResponse, usize), ProtocolError> {
    let (body, consumed) = try_decode_frame(buffer)?;
    let response: DaemonResponse =
        serde_json::from_slice(body).map_err(|error| ProtocolError::Json(error.to_string()))?;
    let version = match &response {
        DaemonResponse::Ok {
            protocol_version, ..
        }
        | DaemonResponse::Err {
            protocol_version, ..
        } => *protocol_version,
    };
    if version != DAEMON_PROTOCOL_VERSION {
        return Err(ProtocolError::VersionMismatch {
            found: version,
            expected: DAEMON_PROTOCOL_VERSION,
        });
    }
    Ok((response, consumed))
}

#[derive(Debug, Deserialize)]
struct AnswerRequestWire {
    protocol_version: u32,
    method: String,
    #[serde(default)]
    id: Option<String>,
    params: AnswerParams,
}

#[cfg(test)]
#[path = "daemon_protocol_tests.rs"]
mod tests;
