//! Daemon UDS serve：单连接 answer 往返与模型缺失映射测试。

use super::*;
use crate::adapters::daemon_instance::{acquire_daemon_lock, prepare_socket_path, DaemonEndpoints};
use crate::domain::daemon_protocol::{
    decode_response, encode_json_frame, AnswerParams, DaemonErrorKind, DaemonRequest,
    DaemonResponse,
};
use crate::domain::published_language::{
    CalibrationLevel, ScoringAnswer, ScoringQuestion, ScoringState,
};
use crate::domain::{ScoringUnavailable, UnavailableKind};
use crate::ports::ScoringPort;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

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

struct FailPort;

#[async_trait]
impl ScoringPort for FailPort {
    async fn answer(
        &self,
        _state: &ScoringState,
        _questions: &[ScoringQuestion],
    ) -> Result<Vec<ScoringAnswer>, ScoringUnavailable> {
        Err(ScoringUnavailable::new(UnavailableKind::Server, "boom"))
    }
}

fn sample_request() -> DaemonRequest {
    DaemonRequest::answer(
        Some("id-1".to_string()),
        AnswerParams {
            scenario: Some("memory_rerank".to_string()),
            state: ScoringState::new("上下文。").expect("state"),
            questions: vec![ScoringQuestion::noul("命题？", None).expect("q")],
        },
    )
}

#[tokio::test]
async fn handle_framed_request_returns_ok_answers() {
    let answer = ScoringAnswer::noul(0.9, CalibrationLevel::Raw).expect("answer");
    let port: Arc<dyn ScoringPort> = Arc::new(FixedPort {
        answers: vec![answer.clone()],
    });
    let request_frame = encode_json_frame(&sample_request()).expect("req");
    let response_frame = handle_framed_request(port.as_ref(), "kev-test", &request_frame)
        .await
        .expect("handle");
    let (response, _) = decode_response(&response_frame).expect("decode");
    match response {
        DaemonResponse::Ok { id, result, .. } => {
            assert_eq!(id.as_deref(), Some("id-1"));
            assert_eq!(result.engine_revision, "kev-test");
            assert_eq!(result.answers, vec![answer]);
        }
        DaemonResponse::Err { .. } => panic!("expected ok"),
    }
}

#[tokio::test]
async fn handle_framed_request_maps_scoring_unavailable_to_error_kind() {
    let port: Arc<dyn ScoringPort> = Arc::new(FailPort);
    let request_frame = encode_json_frame(&sample_request()).expect("req");
    let response_frame = handle_framed_request(port.as_ref(), "kev-test", &request_frame)
        .await
        .expect("handle");
    let (response, _) = decode_response(&response_frame).expect("decode");
    match response {
        DaemonResponse::Err { error, .. } => {
            assert_eq!(error.kind, DaemonErrorKind::Unavailable);
            assert!(error.message.contains("boom"));
        }
        DaemonResponse::Ok { .. } => panic!("expected err"),
    }
}

#[tokio::test]
async fn serve_one_connection_over_unix_socket_round_trips() {
    let temp = tempfile::tempdir().expect("temp");
    let endpoints = DaemonEndpoints {
        scoring_dir: temp.path().to_path_buf(),
        sock: temp.path().join("scoring.sock"),
        lock: temp.path().join("scoring.lock"),
    };
    let lock = acquire_daemon_lock(&endpoints.lock).expect("lock");
    prepare_socket_path(&endpoints.sock, &lock).expect("prepare");

    let answer = ScoringAnswer::noul(0.5, CalibrationLevel::Raw).expect("answer");
    let port: Arc<dyn ScoringPort> = Arc::new(FixedPort {
        answers: vec![answer.clone()],
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let sock = endpoints.sock.clone();
    let server = tokio::spawn(async move {
        run_daemon_accept_loop(endpoints, lock, port, "kev-test".to_string(), shutdown_rx).await
    });

    // 等待 listener 就绪。
    for _ in 0..50 {
        if sock.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let mut stream = UnixStream::connect(&sock).await.expect("connect");
    let request_frame = encode_json_frame(&sample_request()).expect("req");
    stream.write_all(&request_frame).await.expect("write");

    let mut length_buf = [0u8; 4];
    stream.read_exact(&mut length_buf).await.expect("len");
    let length = u32::from_le_bytes(length_buf) as usize;
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body).await.expect("body");
    let mut frame = length_buf.to_vec();
    frame.extend_from_slice(&body);
    let (response, _) = decode_response(&frame).expect("decode");
    match response {
        DaemonResponse::Ok { result, .. } => {
            assert_eq!(result.answers, vec![answer]);
        }
        DaemonResponse::Err { .. } => panic!("expected ok"),
    }

    // 关闭客户端连接，否则 serve_connection 阻塞在 read，accept 循环无法处理 shutdown。
    drop(stream);

    let _ = shutdown_tx.send(());
    server.await.expect("join").expect("serve ok");
}
