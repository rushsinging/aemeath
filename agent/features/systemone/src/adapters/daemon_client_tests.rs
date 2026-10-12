//! DaemonScoringClient：UDS 客户端 ScoringPort 行为测试。

use super::*;
use crate::adapters::daemon_instance::{acquire_daemon_lock, prepare_socket_path, DaemonEndpoints};
use crate::adapters::daemon_serve::run_daemon_accept_loop;
use crate::domain::published_language::{
    CalibrationLevel, ScoringAnswer, ScoringQuestion, ScoringState,
};
use crate::domain::{ScoringUnavailable, UnavailableKind};
use crate::ports::ScoringPort;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

struct FixedPort {
    answers: Vec<ScoringAnswer>,
}

#[async_trait]
impl ScoringPort for FixedPort {
    async fn answer(
        &self,
        _state: &ScoringState,
        _questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        Ok(self.answers.clone())
    }
}

struct SlowPort;

#[async_trait]
impl ScoringPort for SlowPort {
    async fn answer(
        &self,
        _state: &ScoringState,
        _questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        tokio::time::sleep(Duration::from_secs(5)).await;
        Ok(vec![])
    }
}

fn sample_state() -> ScoringState {
    ScoringState::new("评分上下文。").expect("state")
}

fn sample_questions() -> Vec<ScoringQuestion> {
    vec![ScoringQuestion::noul("命题是否为真？", None).expect("q")]
}

#[tokio::test]
async fn missing_sock_maps_to_connect_unavailable() {
    let client = DaemonScoringClient::new(
        std::path::PathBuf::from("/tmp/aemeath-scoring-client-missing.sock"),
        Duration::from_millis(50),
        Duration::from_secs(1),
    );
    let err = client
        .answer(&sample_state(), &sample_questions())
        .await
        .expect_err("sock 不存在应 Unavailable");
    assert_eq!(err.kind(), UnavailableKind::Connect);
}

#[tokio::test]
async fn answer_round_trips_via_local_daemon() {
    let temp = tempfile::tempdir().expect("temp");
    let endpoints = DaemonEndpoints {
        scoring_dir: temp.path().to_path_buf(),
        sock: temp.path().join("scoring.sock"),
        lock: temp.path().join("scoring.lock"),
    };
    let lock = acquire_daemon_lock(&endpoints.lock).expect("lock");
    prepare_socket_path(&endpoints.sock, &lock).expect("prepare");

    let answer = ScoringAnswer::noul(0.81, CalibrationLevel::Temperature).expect("answer");
    let port: Arc<dyn ScoringPort> = Arc::new(FixedPort {
        answers: vec![answer.clone()],
    });
    let sock = endpoints.sock.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        run_daemon_accept_loop(
            endpoints,
            lock,
            port,
            "kev-client-test".to_string(),
            shutdown_rx,
        )
        .await
    });

    for _ in 0..50 {
        if sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let client =
        DaemonScoringClient::new(sock.clone(), Duration::from_secs(1), Duration::from_secs(2));
    let answers = client
        .answer(&sample_state(), &sample_questions())
        .await
        .expect("应成功");
    assert_eq!(answers, vec![answer]);

    let _ = shutdown_tx.send(());
    let _ = server.await;
}

#[tokio::test]
async fn infer_timeout_maps_to_timeout_unavailable() {
    let temp = tempfile::tempdir().expect("temp");
    let endpoints = DaemonEndpoints {
        scoring_dir: temp.path().to_path_buf(),
        sock: temp.path().join("scoring.sock"),
        lock: temp.path().join("scoring.lock"),
    };
    let lock = acquire_daemon_lock(&endpoints.lock).expect("lock");
    prepare_socket_path(&endpoints.sock, &lock).expect("prepare");

    let port: Arc<dyn ScoringPort> = Arc::new(SlowPort);
    let sock = endpoints.sock.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        run_daemon_accept_loop(endpoints, lock, port, "kev-slow".to_string(), shutdown_rx).await
    });

    for _ in 0..50 {
        if sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let client = DaemonScoringClient::new(sock, Duration::from_secs(1), Duration::from_millis(80));
    let err = client
        .answer(&sample_state(), &sample_questions())
        .await
        .expect_err("推理超时应 Unavailable");
    assert_eq!(err.kind(), UnavailableKind::Timeout);

    let _ = shutdown_tx.send(());
    let _ = server.await;
}
