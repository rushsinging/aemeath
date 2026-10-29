//! LocalSystemOneClient L2 测试：对 daemon_http_serve 真 server 做 loopback 端到端。

use super::*;
use crate::adapters::daemon_http_serve::serve_http_on;
use crate::domain::CalibrationLevel;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

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

async fn spawn_real_daemon(
    answer: Result<Vec<ScoringAnswer>, ScoringUnavailable>,
) -> (String, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener bind");
    let address = listener.local_addr().expect("listener addr");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        let _ = serve_http_on(
            listener,
            Arc::new(StubPort { answer }),
            "kev-0.8b-q8-r1".to_string(),
            shutdown_rx,
        )
        .await;
    });
    (format!("http://{address}/systemone"), shutdown_tx)
}

fn sample_input() -> (ScoringState, Vec<ScoringQuestion>) {
    let state = ScoringState::new("用户问：如何预防感冒？").expect("state");
    let question = ScoringQuestion::choice(
        "哪条记忆最相关？",
        vec![
            ("m1".to_string(), "关于预防感冒的卫生习惯".to_string()),
            ("m2".to_string(), "Rust 内存安全特性".to_string()),
        ],
    )
    .expect("choice");
    (state, vec![question])
}

#[tokio::test]
async fn local_client_answers_against_real_daemon_http_serve() {
    let (endpoint, shutdown) = spawn_real_daemon(Ok(vec![ScoringAnswer::choice(
        "m1",
        vec![("m1".to_string(), 0.9), ("m2".to_string(), 0.1)],
        0.9,
        CalibrationLevel::Raw,
    )
    .expect("choice")]))
    .await;
    let client =
        LocalSystemOneClient::new(endpoint, "kev-local".to_string(), Duration::from_secs(5));
    let (state, questions) = sample_input();
    let answers = client
        .answer(&state, &questions)
        .await
        .expect("真 daemon 调用应成功");
    assert_eq!(answers.len(), 1);
    assert!(matches!(
        &answers[0],
        ScoringAnswer::Choice { choice, .. } if choice == "m1"
    ));
    let _ = shutdown.send(());
}

#[tokio::test]
async fn local_client_maps_daemon_503_to_server_kind() {
    let (endpoint, shutdown) = spawn_real_daemon(Err(ScoringUnavailable::new(
        UnavailableKind::Server,
        "worker 故障",
    )))
    .await;
    let client =
        LocalSystemOneClient::new(endpoint, "kev-local".to_string(), Duration::from_secs(5));
    let (state, questions) = sample_input();
    let error = client
        .answer(&state, &questions)
        .await
        .expect_err("daemon 503 应映射为不可用");
    assert_eq!(error.kind(), UnavailableKind::Server);
    let _ = shutdown.send(());
}

#[tokio::test]
async fn local_client_maps_connection_failure_to_transport_kind() {
    let client = LocalSystemOneClient::new(
        "http://127.0.0.1:1/systemone".to_string(),
        "kev-local".to_string(),
        Duration::from_millis(500),
    );
    let (state, questions) = sample_input();
    let error = client
        .answer(&state, &questions)
        .await
        .expect_err("无监听端口应映射为不可用");
    assert!(
        matches!(
            error.kind(),
            UnavailableKind::Connect | UnavailableKind::Timeout
        ),
        "传输层失败归传输类 kind：{error}"
    );
}

// #2146 密封解锁。
impl crate::sealed::Sealed for StubPort {}
