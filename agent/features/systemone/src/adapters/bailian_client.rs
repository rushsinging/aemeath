//! 云端 System One 决策评估 HTTPS 客户端：百炼与火山方舟（beta）两个独立
//! 类型，共享私有传输层 [`SystemOneHttpTransport`]。
//!
//! 设计依据：`docs/design/02-modules/systemone/01-systemone-scoring.md`（#2095）。
//! - 端点 = 配置的 `full_url`（优先）或 `base_url + "/systemone"`；
//! - Bearer 鉴权与模型名来自评分配置的独立段（bailian / volcengine），
//!   **NEVER** 读 `models.providers`；
//! - 火山方舟 System One 处于 beta，请求固定携带 `ark-beta-systemone-api: true`；
//! - 传输/协议错误一律映射 [`ScoringUnavailable`]（消费点按原有词法/启发式
//!   路径降级，不阻断主循环，设计 §4.4）。

use std::time::Duration;

use crate::constants::VOLCENGINE_BETA_HEADER;
use crate::domain::error::{ScoringUnavailable, UnavailableKind};
use crate::domain::systemone_http_wire::{self, SystemOneHttpResponse};
use crate::domain::{ScoringAnswer, ScoringQuestion, ScoringState};
use crate::ports::ScoringPort;

/// 云端 System One 共享传输层：两个后端只在构造期注入不同的端点、凭证与
/// 额外请求头，请求/响应语义完全一致。
#[derive(Debug, Clone)]
struct SystemOneHttpTransport {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
    infer_timeout: Duration,
    extra_headers: Vec<(String, String)>,
}

impl SystemOneHttpTransport {
    fn new(
        endpoint: String,
        api_key: String,
        model: String,
        infer_timeout: Duration,
        extra_headers: Vec<(String, String)>,
    ) -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap_or_default(),
            endpoint,
            api_key,
            model,
            infer_timeout,
            extra_headers,
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

    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        let request_body = systemone_http_wire::build_request(&self.model, state, questions);
        let mut request = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .timeout(self.infer_timeout);
        for (name, value) in &self.extra_headers {
            request = request.header(name, value);
        }
        let response = request
            .json(&request_body)
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

/// 百炼 System One HTTPS 客户端。
#[derive(Debug, Clone)]
pub struct BailianSystemOneClient {
    transport: SystemOneHttpTransport,
}

impl BailianSystemOneClient {
    /// 构造客户端；`endpoint` 为完整请求 URL（含 `/systemone` 路径）。
    pub fn new(endpoint: String, api_key: String, model: String, infer_timeout: Duration) -> Self {
        Self {
            transport: SystemOneHttpTransport::new(
                endpoint,
                api_key,
                model,
                infer_timeout,
                Vec::new(),
            ),
        }
    }
}

/// 火山方舟 System One 决策评估客户端（beta）。
///
/// 与百炼客户端**独立类型**：构造期固定注入 `ark-beta-systemone-api: true`，
/// 不向调用方暴露额外请求头，避免 beta 头部漏发导致 4xx。
#[derive(Debug, Clone)]
pub struct VolcengineSystemOneClient {
    transport: SystemOneHttpTransport,
}

impl VolcengineSystemOneClient {
    /// 构造客户端；`endpoint` 为完整请求 URL（含 `/systemone` 路径）。
    pub fn new(endpoint: String, api_key: String, model: String, infer_timeout: Duration) -> Self {
        Self {
            transport: SystemOneHttpTransport::new(
                endpoint,
                api_key,
                model,
                infer_timeout,
                vec![(
                    VOLCENGINE_BETA_HEADER.0.to_string(),
                    VOLCENGINE_BETA_HEADER.1.to_string(),
                )],
            ),
        }
    }
}

/// 沿 source 链寻找超时特征（tokio `Elapsed` 的稳定消息为 timed out）。
fn error_chain_indicates_timeout(error: &reqwest::Error) -> bool {
    let mut source = std::error::Error::source(error);
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
        self.transport.answer(state, questions).await
    }
}

#[async_trait::async_trait]
impl ScoringPort for VolcengineSystemOneClient {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        self.transport.answer(state, questions).await
    }
}

#[cfg(test)]
#[path = "bailian_client_tests.rs"]
mod tests;
