//! 百炼决策模型（TypeSafe System One 协议）HTTPS 客户端：实现 [`ScoringPort`]。
//!
//! 设计依据：`docs/design/02-modules/systemone/01-systemone-scoring.md`（#2095）。
//! - 端点 = `scoring.bailian` 的 `full_url`（优先）或 `base_url + "/systemone"`；
//! - Bearer 鉴权与模型名来自 `scoring.bailian` 独立配置，**NEVER** 读
//!   `models.providers`；
//! - 传输/协议错误一律映射 [`ScoringUnavailable`]（消费点按原有词法/启发式
//!   路径降级，不阻断主循环，设计 §4.4）。

use std::time::Duration;

use crate::domain::error::{ScoringUnavailable, UnavailableKind};
use crate::domain::systemone_http_wire::{self, SystemOneHttpResponse};
use crate::domain::{ScoringAnswer, ScoringQuestion, ScoringState};
use crate::ports::ScoringPort;

/// 百炼 System One HTTPS 客户端。
#[derive(Debug, Clone)]
pub struct BailianSystemOneClient {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
    infer_timeout: Duration,
}

impl BailianSystemOneClient {
    /// 构造客户端；`endpoint` 为完整请求 URL（含 `/systemone` 路径）。
    pub fn new(endpoint: String, api_key: String, model: String, infer_timeout: Duration) -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap_or_default(),
            endpoint,
            api_key,
            model,
            infer_timeout,
        }
    }

    fn transport_error(error: reqwest::Error) -> ScoringUnavailable {
        // reqwest 0.12 的 DNS 解析失败也报 `is_timeout()==true`（实测探针
        // 证据）；真超时须沿 source 链确认（tokio `Elapsed` / "timed out"），
        // 其余传输错误（连接拒绝、DNS、TLS）一律归 Connect。
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
impl ScoringPort for BailianSystemOneClient {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        let request_body = systemone_http_wire::build_request(&self.model, state, questions);
        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .timeout(self.infer_timeout)
            .send()
            .await
            .map_err(Self::transport_error)?;
        let status = response.status();
        if !status.is_success() {
            let body_detail = response.text().await.unwrap_or_default();
            // detail 截断防日志爆炸（对齐 daemon client 语义）。
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
#[path = "bailian_client_tests.rs"]
mod tests;

// #2146 密封解锁。
impl crate::sealed::Sealed for BailianSystemOneClient {}
