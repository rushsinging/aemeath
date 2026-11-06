//! local daemon HTTP 客户端（#2096：UDS 退役，HTTP 替代）。
//!
//! 与 `BailianSystemOneClient` **独立类型**（用户拍板：client NEVER 泛化，
//! 未来其他 web 提供商各自独立）；wire DTO 与领域转换共享
//! `systemone_http_wire`。明文 HTTP，**无鉴权头**（与 daemon 端约定一致）。
//! 错误全部映射 `ScoringUnavailable`，消费点按设计 §4.4 统一降级。

use std::time::Duration;

use crate::domain::error::{ScoringUnavailable, UnavailableKind};
use crate::domain::systemone_http_wire::{self, SystemOneHttpResponse};
use crate::domain::{ScoringAnswer, ScoringQuestion, ScoringState};
use crate::ports::ScoringPort;

/// local daemon HTTP 客户端。
#[derive(Debug, Clone)]
pub struct LocalSystemOneClient {
    http: reqwest::Client,
    endpoint: String,
    model: String,
    infer_timeout: Duration,
}

impl LocalSystemOneClient {
    /// 构造客户端；`endpoint` 为完整请求 URL（含 `/systemone` 路径）；
    /// `model` 为请求回显字段（daemon 侧按本地引擎 revision 回显，不强制匹配）。
    pub fn new(endpoint: String, model: String, infer_timeout: Duration) -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap_or_default(),
            endpoint,
            model,
            infer_timeout,
        }
    }

    fn transport_error(error: reqwest::Error) -> ScoringUnavailable {
        let kind = if error.is_timeout() && error_chain_indicates_timeout(&error) {
            UnavailableKind::Timeout
        } else {
            UnavailableKind::Connect
        };
        ScoringUnavailable::new(kind, error.to_string())
    }
}

/// 沿 source 链寻找超时特征（tokio `Elapsed` 的稳定消息为 timed out）。
fn error_chain_indicates_timeout(error: &reqwest::Error) -> bool {
    let mut source = std::error::Error::source(&error);
    while let Some(current) = source {
        let detail = current.to_string();
        if detail.contains("timed out") || detail.contains("operation was canceled") {
            return true;
        }
        source = current.source();
    }
    false
}

#[async_trait::async_trait]
impl ScoringPort for LocalSystemOneClient {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        let request_body = systemone_http_wire::build_request(&self.model, state, questions);
        let response = self
            .http
            .post(&self.endpoint)
            .json(&request_body)
            .timeout(self.infer_timeout)
            .send()
            .await
            .map_err(Self::transport_error)?;
        let status = response.status();
        if !status.is_success() {
            let body_detail = response.text().await.unwrap_or_default();
            let detail = format!(
                "HTTP {status}：{}",
                body_detail.chars().take(300).collect::<String>()
            );
            let kind = if status.is_server_error() {
                UnavailableKind::Server
            } else {
                UnavailableKind::Client
            };
            return Err(ScoringUnavailable::new(kind, detail));
        }
        let wire_response: SystemOneHttpResponse = response.json().await.map_err(|error| {
            ScoringUnavailable::new(UnavailableKind::Schema, format!("响应 JSON 非法：{error}"))
        })?;
        systemone_http_wire::parse_answers(&wire_response, questions).map_err(|wire_error| {
            ScoringUnavailable::new(UnavailableKind::Schema, wire_error.to_string())
        })
    }
}

#[cfg(test)]
#[path = "local_client_tests.rs"]
mod tests;
