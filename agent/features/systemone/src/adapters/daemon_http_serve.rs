//! Scoring daemon HTTP accept loop（#2096：UDS 退役，HTTP 替代）。
//!
//! 设计依据：`docs/design/02-modules/systemone/04-scoring-daemon.md` §4–§5。
//! - `POST /systemone`：System One JSON（与 bailian 同构，`systemone_http_wire`
//!   共享 DTO）→ `ScoringPort::answer` → 同构响应；
//! - `GET /healthz`：引擎就绪探活（serve 启动即引擎就绪，恒 200）；
//! - 单实例由 listen 地址 bind 互斥保证（bind 失败即第二实例，上抛 io 错误）；
//! - 明文 HTTP，无鉴权（用户拍板：部署暴露面由配置负责）。

use std::io;
use std::sync::Arc;
use std::time::Instant;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::domain::systemone_http_wire::{build_response, parse_request, SystemOneHttpRequest};
use crate::ports::ScoringPort;

type HttpBody = Full<Bytes>;
type HttpResult = Result<Response<HttpBody>, hyper::Error>;

struct DaemonHttpState {
    port: Arc<dyn ScoringPort>,
    engine_revision: String,
}

/// 绑定 `listen`（`host:port`）并循环 accept；`shutdown` 触发后优雅退出。
pub async fn run_daemon_http_serve(
    listen: &str,
    port: Arc<dyn ScoringPort>,
    engine_revision: String,
    mut shutdown: oneshot::Receiver<()>,
) -> io::Result<()> {
    let listener = TcpListener::bind(listen).await?;
    let state = Arc::new(DaemonHttpState {
        port,
        engine_revision,
    });

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let connection_state = Arc::clone(&state);
                tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        let handler_state = Arc::clone(&connection_state);
                        async move { handle_request(handler_state, request).await }
                    });
                    let connection = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service);
                    if let Err(error) = connection.await {
                        log::warn!(
                            target: crate::LOG_TARGET,
                            "scoring daemon HTTP 连接处理失败：{error}"
                        );
                    }
                });
            }
        }
    }
    Ok(())
}

async fn handle_request(
    state: Arc<DaemonHttpState>,
    request: Request<hyper::body::Incoming>,
) -> HttpResult {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    match (method, path.as_str()) {
        (Method::GET, "/healthz") => Ok(json_response(StatusCode::OK, r#"{"status":"ok"}"#)),
        (Method::POST, "/systemone") => {
            let body = match request.into_body().collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(error) => {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        &format!(
                            r#"{{"error":{{"kind":"protocol","message":"请求体读取失败：{error}"}}}}"#
                        ),
                    ));
                }
            };
            let request_wire: SystemOneHttpRequest = match serde_json::from_slice(&body) {
                Ok(parsed) => parsed,
                Err(error) => {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        &format!(
                            r#"{{"error":{{"kind":"protocol","message":"请求 JSON 非法：{error}"}}}}"#
                        ),
                    ));
                }
            };
            let (scoring_state, questions) = match parse_request(&request_wire) {
                Ok(parsed) => parsed,
                Err(error) => {
                    return Ok(json_response(
                        StatusCode::BAD_REQUEST,
                        &format!(
                            r#"{{"error":{{"kind":"protocol","message":"请求语义非法：{error}"}}}}"#
                        ),
                    ));
                }
            };
            let started = Instant::now();
            match state.port.answer(&scoring_state, &questions).await {
                Ok(answers) => {
                    let response_wire = build_response(
                        &state.engine_revision,
                        &answers,
                        started.elapsed().as_secs_f64() * 1000.0,
                    );
                    let body = serde_json::to_string(&response_wire).unwrap_or_else(|_| {
                        r#"{"error":{"kind":"internal","message":"响应序列化失败"}}"#.to_string()
                    });
                    Ok(json_response(StatusCode::OK, &body))
                }
                Err(unavailable) => Ok(json_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    &format!(
                        r#"{{"error":{{"kind":"unavailable","message":"{}"}}}}"#,
                        unavailable
                    ),
                )),
            }
        }
        _ => Ok(json_response(
            StatusCode::NOT_FOUND,
            r#"{"error":{"kind":"protocol","message":"未知路径"}}"#,
        )),
    }
}

fn json_response(status: StatusCode, body: &str) -> Response<HttpBody> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(HttpBody::new(Bytes::from(body.to_string())))
        .expect("响应构造")
}

#[cfg(test)]
#[path = "daemon_http_serve_tests.rs"]
mod tests;
