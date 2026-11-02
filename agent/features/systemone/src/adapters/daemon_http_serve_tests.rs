//! daemon HTTP serve L2 契约测试：真 listener + reqwest 客户端。

use super::*;
use crate::domain::systemone_http_wire::{build_request, parse_answers, SystemOneHttpResponse};
use crate::domain::{
    CalibrationLevel, ScoringAnswer, ScoringQuestion, ScoringState, ScoringUnavailable,
};
use std::time::Duration;

struct StubPort {
    answer: Result<Vec<ScoringAnswer>, ScoringUnavailable>,
}

#[async_trait::async_trait]
impl ScoringPort for StubPort {
    async fn answer(
        &self,
        _state: &ScoringState,
        _questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        self.answer.clone()
    }
}

fn choice_answer() -> Vec<ScoringAnswer> {
    vec![ScoringAnswer::choice(
        "m1",
        vec![("m1".to_string(), 0.9), ("m2".to_string(), 0.1)],
        0.9,
        CalibrationLevel::Raw,
    )
    .expect("choice 构造")]
}

async fn spawn_serve(
    port: Arc<dyn ScoringPort>,
) -> (
    String,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<io::Result<()>>,
) {
    // 直接持有已 bind 的 listener（测试随机端口；生产端口显式配置），
    // 消除 bind-drop-rebind 端口竞态。
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener bind");
    let address = listener.local_addr().expect("listener addr");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        serve_http_on(listener, port, "kev-0.8b-q8-r1".to_string(), shutdown_rx).await
    });
    (format!("http://{address}"), shutdown_tx, handle)
}

fn sample_request() -> SystemOneHttpRequest {
    let state = ScoringState::new("用户问：如何预防感冒？").expect("state");
    let question = ScoringQuestion::choice(
        "哪条记忆最相关？",
        vec![
            ("m1".to_string(), "关于预防感冒的卫生习惯".to_string()),
            ("m2".to_string(), "Rust 内存安全特性".to_string()),
        ],
    )
    .expect("choice 构造");
    build_request("kev-0.8b-q8-r1", &state, &[question])
}

#[tokio::test]
async fn http_serve_answers_systemone_request_with_shared_wire_shape() {
    let (base_url, shutdown, _handle) = spawn_serve(Arc::new(StubPort {
        answer: Ok(choice_answer()),
    }))
    .await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base_url}/systemone"))
        .json(&sample_request())
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .expect("HTTP 调用");
    assert_eq!(response.status(), 200);
    let wire: SystemOneHttpResponse = response.json().await.expect("响应解析");
    assert_eq!(wire.model, "kev-0.8b-q8-r1");
    let questions = vec![ScoringQuestion::choice(
        "哪条记忆最相关？",
        vec![
            ("m1".to_string(), "关于预防感冒的卫生习惯".to_string()),
            ("m2".to_string(), "Rust 内存安全特性".to_string()),
        ],
    )
    .expect("choice")];
    let answers = parse_answers(&wire, &questions).expect("client 还原");
    assert!(matches!(
        &answers[0],
        ScoringAnswer::Choice { choice, .. } if choice == "m1"
    ));
    let _ = shutdown.send(());
}

#[tokio::test]
async fn http_serve_healthz_returns_ok() {
    let (base_url, shutdown, _handle) = spawn_serve(Arc::new(StubPort {
        answer: Ok(choice_answer()),
    }))
    .await;
    let response = reqwest::get(format!("{base_url}/healthz"))
        .await
        .expect("healthz 调用");
    assert_eq!(response.status(), 200);
    let _ = shutdown.send(());
}

#[tokio::test]
async fn http_serve_rejects_malformed_json_with_400() {
    let (base_url, shutdown, _handle) = spawn_serve(Arc::new(StubPort {
        answer: Ok(choice_answer()),
    }))
    .await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base_url}/systemone"))
        .body("not-json")
        .header("content-type", "application/json")
        .send()
        .await
        .expect("HTTP 调用");
    assert_eq!(response.status(), 400);
    let _ = shutdown.send(());
}

#[tokio::test]
async fn http_serve_maps_scoring_unavailable_to_503() {
    let (base_url, shutdown, _handle) = spawn_serve(Arc::new(StubPort {
        answer: Err(ScoringUnavailable::new(
            crate::domain::UnavailableKind::Server,
            "worker 故障",
        )),
    }))
    .await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base_url}/systemone"))
        .json(&sample_request())
        .send()
        .await
        .expect("HTTP 调用");
    assert_eq!(response.status(), 503);
    let _ = shutdown.send(());
}

#[tokio::test]
async fn http_serve_unknown_path_returns_404() {
    let (base_url, shutdown, _handle) = spawn_serve(Arc::new(StubPort {
        answer: Ok(choice_answer()),
    }))
    .await;
    let response = reqwest::get(format!("{base_url}/other"))
        .await
        .expect("未知路径调用");
    assert_eq!(response.status(), 404);
    let _ = shutdown.send(());
}

// #2146 密封解锁。
impl crate::sealed::Sealed for StubPort {}
