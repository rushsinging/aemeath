//! BailianSystemOneClient L2 测试：本地 mock HTTP server 覆盖传输行为与错误映射。

use super::*;
use crate::domain::{ScoringQuestion, ScoringState, UnavailableKind};

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct MockBailianServer {
    endpoint: String,
    requests: tokio::sync::mpsc::UnboundedReceiver<String>,
    _task: tokio::task::JoinHandle<()>,
}

async fn start_mock_server(status: u16, body: &'static str) -> MockBailianServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定端口");
    let address = listener.local_addr().expect("本地地址");
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buffer = vec![0u8; 64 * 1024];
            let mut request = Vec::new();
            loop {
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let header = String::from_utf8_lossy(&request[..header_end]).to_string();
                    let content_length = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + content_length {
                        break;
                    }
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&request).to_string());
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {length}\r\nconnection: close\r\n\r\n{body}",
                reason = if (200..300).contains(&status) { "OK" } else { "ERR" },
                length = body.len(),
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    MockBailianServer {
        endpoint: format!("http://{address}/systemone"),
        requests: receiver,
        _task: task,
    }
}

fn client_for(endpoint: &str) -> BailianSystemOneClient {
    BailianSystemOneClient::new(
        endpoint.to_string(),
        "sk-test-key".to_string(),
        "decision-model-preview".to_string(),
        Duration::from_millis(1500),
    )
}

fn noul_question() -> ScoringQuestion {
    ScoringQuestion::noul("这段文本是否为中文？", None).expect("question 构造")
}

fn sample_state() -> ScoringState {
    ScoringState::new("用户正在调试日志路由。").expect("state 构造")
}

/// 实测响应原文（noul）。
const OK_NOUL_BODY: &str = r#"{"model":"decision-model-preview","request_id":"ab2602c0","answers":{"q0":{"type":"noul","noul":1.0}},"usage":{"input_tokens":23},"latency_ms":39.1}"#;

#[tokio::test]
async fn answer_posts_bearer_authorized_request_and_maps_noul_answer() {
    let mut server = start_mock_server(200, OK_NOUL_BODY).await;
    let client = client_for(&server.endpoint);

    let answers = client
        .answer(&sample_state(), &[noul_question()])
        .await
        .expect("200 响应应解析成功");

    assert_eq!(answers.len(), 1);
    assert!(matches!(
        &answers[0],
        crate::domain::ScoringAnswer::Noul { p_true, .. } if *p_true == 1.0
    ));

    let request_text = server.requests.recv().await.expect("请求应到达 server");
    assert!(
        request_text.starts_with("POST /systemone "),
        "请求应 POST 端点路径：{request_text}"
    );
    assert!(
        request_text
            .to_ascii_lowercase()
            .contains("authorization: bearer sk-test-key"),
        "必须携带 Bearer 鉴权头"
    );
    assert!(
        request_text.contains("\"model\":\"decision-model-preview\""),
        "请求体应携带 model"
    );
    assert!(
        request_text.contains("\"q0\""),
        "请求体应携带 qid=q0 的题目"
    );
}

#[tokio::test]
async fn answer_maps_4xx_to_client_kind_with_status_detail() {
    let server = start_mock_server(401, r#"{"error":{"code":"AuthenticationError"}}"#).await;
    let client = client_for(&server.endpoint);
    let error = client
        .answer(&sample_state(), &[noul_question()])
        .await
        .expect_err("401 必须映射为不可用");
    assert_eq!(error.kind(), UnavailableKind::Client);
    assert!(
        error.to_string().contains("401"),
        "detail 应包含状态码：{error}"
    );
}

#[tokio::test]
async fn answer_maps_5xx_to_server_kind() {
    let server = start_mock_server(503, r#"{"error":"busy"}"#).await;
    let client = client_for(&server.endpoint);
    let error = client
        .answer(&sample_state(), &[noul_question()])
        .await
        .expect_err("503 必须映射为不可用");
    assert_eq!(error.kind(), UnavailableKind::Server);
}

#[tokio::test]
async fn answer_maps_malformed_body_to_schema_kind() {
    let server = start_mock_server(200, "not-json").await;
    let client = client_for(&server.endpoint);
    let error = client
        .answer(&sample_state(), &[noul_question()])
        .await
        .expect_err("畸形 JSON 必须映射为不可用");
    assert_eq!(error.kind(), UnavailableKind::Schema);
}

#[tokio::test]
async fn answer_maps_wire_mismatch_to_schema_kind() {
    // 响应缺 q0 答案（qid 不匹配）。
    let body = r#"{"model":"m","request_id":"r","answers":{},"usage":{"input_tokens":1},"latency_ms":1.0}"#;
    let server = start_mock_server(200, body).await;
    let client = client_for(&server.endpoint);
    let error = client
        .answer(&sample_state(), &[noul_question()])
        .await
        .expect_err("缺答案必须映射为不可用");
    assert_eq!(error.kind(), UnavailableKind::Schema);
    assert!(
        error.to_string().contains("q0"),
        "detail 应指出缺失 qid：{error}"
    );
}

#[tokio::test]
async fn answer_maps_timeout_to_timeout_kind() {
    // 挂起连接的 server：接受连接但永不响应。
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定端口");
    let address = listener.local_addr().expect("本地地址");
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            // 持有连接不响应，直到对端超时断开。
            let _held = socket;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    let client = BailianSystemOneClient::new(
        format!("http://{address}/systemone"),
        "sk-test-key".to_string(),
        "decision-model-preview".to_string(),
        Duration::from_millis(200),
    );
    let error = client
        .answer(&sample_state(), &[noul_question()])
        .await
        .expect_err("超时必须映射为不可用");
    assert_eq!(error.kind(), UnavailableKind::Timeout);
}

#[tokio::test]
async fn answer_maps_connection_failure_to_transport_kind() {
    // `.invalid` 是 RFC 2606 保留 TLD，保证解析失败。reqwest 0.12 对 DNS
    // 失败 / 连接拒绝可能统一呈现为超时链（实测探针证据），断言放宽为
    // 传输类归因（Connect | Timeout），NEVER 误归 Server/Schema。
    let client = BailianSystemOneClient::new(
        "https://bailian-unreachable.invalid/systemone".to_string(),
        "sk-test-key".to_string(),
        "decision-model-preview".to_string(),
        Duration::from_millis(500),
    );
    let error = client
        .answer(&sample_state(), &[noul_question()])
        .await
        .expect_err("连接失败必须映射为不可用");
    assert!(
        matches!(
            error.kind(),
            UnavailableKind::Connect | UnavailableKind::Timeout
        ),
        "传输层失败必须归传输类 kind：{error}"
    );
}
