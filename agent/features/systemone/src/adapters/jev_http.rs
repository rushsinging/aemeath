//! Jev HTTP adapter：`POST {base}/v1/systemone` 线格式客户端。
//!
//! 降级语义：连接失败 / 超时 / 5xx / 422（含响应 schema 非法）→ `ScoringUnavailable`；
//! **单次失败不熔断**，由消费点逐次回退。

use std::time::Duration;

use async_trait::async_trait;

use crate::adapters::jev_wire::{parse_answers, WireRequest};
use crate::domain::{
    ScoringAnswer, ScoringQuestion, ScoringState, ScoringUnavailable, UnavailableKind,
};
use crate::ports::ScoringPort;

const SYSTEMONE_PATH: &str = "/v1/systemone";

/// 连接预检窗口：hyper-util legacy client 对可重用 body 的请求在 connect 失败时
/// 会无限重试直至总超时（实测 connect refused 等满 2s 报 timeout）。
/// 评分前先做廉价 TCP 预检（loopback ~100µs），把「服务未启动」这一最常见
/// 不可用形态的回退延迟降到毫秒级；预检通过后的请求仍走完整超时窗口。
const PREFLIGHT_TIMEOUT: Duration = Duration::from_millis(500);

/// Jev 线格式 HTTP adapter（kev / rsi-jev 同协议复用，配置切换）。
pub struct JevHttpScoringAdapter {
    endpoint: String,
    preflight_address: String,
    model: String,
    client: reqwest::Client,
}

impl JevHttpScoringAdapter {
    /// `base_url` 为服务根（如 `http://127.0.0.1:8009`），端点路径由 adapter 拼接。
    ///
    /// # Panics
    ///
    /// `base_url` 非法（无法解析 host）时 panic——配置值在装配期校验，属编程错误。
    pub fn new(base_url: &str, model: impl Into<String>, timeout: Duration) -> Self {
        let trimmed = base_url.trim_end_matches('/');
        let parsed = reqwest::Url::parse(trimmed)
            .unwrap_or_else(|_| panic!("scoring base_url 非法：{base_url}"));
        let host = parsed
            .host_str()
            .unwrap_or_else(|| panic!("scoring base_url 缺 host：{base_url}"));
        let port = parsed.port_or_known_default().unwrap_or(80);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client 构造失败仅可能为 TLS 配置错误");
        Self {
            endpoint: format!("{trimmed}{SYSTEMONE_PATH}"),
            preflight_address: format!("{host}:{port}"),
            model: model.into(),
            client,
        }
    }

    /// 引擎 revision（审计溯源用）：当前为配置的模型名。
    pub fn engine_revision(&self) -> &str {
        &self.model
    }

    /// TCP 预检：服务端口不可达立即失败，规避 hyper-util connect 重试循环。
    async fn preflight_check(&self) -> Result<(), ScoringUnavailable> {
        match tokio::time::timeout(
            PREFLIGHT_TIMEOUT,
            tokio::net::TcpStream::connect(&self.preflight_address),
        )
        .await
        {
            Ok(Ok(_stream)) => Ok(()),
            Ok(Err(error)) => Err(ScoringUnavailable::new(
                UnavailableKind::Connect,
                format!("预检连接失败：{error}"),
            )),
            Err(_) => Err(ScoringUnavailable::new(
                UnavailableKind::Timeout,
                "预检连接超时".to_owned(),
            )),
        }
    }
}

#[async_trait]
impl ScoringPort for JevHttpScoringAdapter {
    async fn answer(
        &self,
        state: &ScoringState,
        questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        if questions.is_empty() {
            return Ok(Vec::new());
        }
        self.preflight_check().await?;
        let indexed: Vec<(String, ScoringQuestion)> = questions
            .iter()
            .enumerate()
            .map(|(index, question)| (format!("q{index}"), question.clone()))
            .collect();
        let body = serde_json::to_string(&WireRequest {
            state,
            model: &self.model,
            questions: &indexed,
        })
        .map_err(|error| {
            ScoringUnavailable::new(UnavailableKind::Schema, format!("请求序列化失败：{error}"))
        })?;

        let response = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| {
                // 预检已覆盖「服务未启动」；请求期失败为服务中途不可达或响应超时，
                // hyper-util connect 重试循环至超时后如实归 Timeout。
                if error.is_timeout() {
                    ScoringUnavailable::new(UnavailableKind::Timeout, error.to_string())
                } else {
                    ScoringUnavailable::new(UnavailableKind::Connect, error.to_string())
                }
            })?;

        let status = response.status();
        if status.is_server_error() {
            return Err(ScoringUnavailable::new(
                UnavailableKind::Server,
                format!("HTTP {status}"),
            ));
        }
        if !status.is_success() {
            return Err(ScoringUnavailable::new(
                UnavailableKind::Schema,
                format!("HTTP {status}"),
            ));
        }

        let text = response.text().await.map_err(|error| {
            ScoringUnavailable::new(UnavailableKind::Schema, format!("响应读取失败：{error}"))
        })?;
        parse_answers(&text, &indexed).map_err(|rejected| {
            ScoringUnavailable::new(UnavailableKind::Schema, rejected.to_string())
        })
    }
}

#[cfg(test)]
#[path = "jev_http_tests.rs"]
mod tests;
