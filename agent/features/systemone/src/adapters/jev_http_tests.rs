//! JevHttpScoringAdapter L2 测试：mock HTTP server 覆盖传输行为与错误映射。

use super::*;
use crate::domain::ScoringState;

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// mock server 行为脚本：每个连接依次返回一段响应；`NeverRespond` 挂起连接。
enum MockBehavior {
    Fixed { status: u16, body: &'static str },
    Sequence(Vec<(u16, &'static str)>),
    NeverRespond,
}

struct MockJevServer {
    base_url: String,
    requests: tokio::sync::mpsc::UnboundedReceiver<String>,
    _task: tokio::task::JoinHandle<()>,
}

async fn start_mock_server(behavior: MockBehavior) -> MockJevServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定端口");
    let address = listener.local_addr().expect("本地地址");
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let mut sequence_index = 0usize;
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buffer = vec![0u8; 64 * 1024];
            let mut request = Vec::new();
            // 读完请求（Content-Length 极简处理：读到 header 结束 + body 长度）。
            loop {
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if let Some(header_end) = find_subsequence(&request, b"\r\n\r\n") {
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
            let request_text = String::from_utf8_lossy(&request).to_string();
            // adapter 的 TCP 预检连接不带任何 payload，跳过不计入请求记录、
            // 不消耗序列、不回响应。
            if request_text.is_empty() {
                continue;
            }
            let _ = sender.send(request_text);
            let (status, body) = match &behavior {
                MockBehavior::Fixed { status, body } => (*status, *body),
                MockBehavior::Sequence(steps) => {
                    let step = steps
                        .get(sequence_index)
                        .or_else(|| steps.last())
                        .copied()
                        .unwrap_or((500, "{}"));
                    sequence_index += 1;
                    step
                }
                MockBehavior::NeverRespond => {
                    // 挂起直至 client 超时断开。
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    return;
                }
            };
            let reason = if (200..300).contains(&status) {
                "OK"
            } else {
                "ERR"
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    MockJevServer {
        base_url: format!("http://{address}"),
        requests: receiver,
        _task: task,
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn noul_question() -> ScoringQuestion {
    ScoringQuestion::noul("任务是否已完成？", None).expect("question 构造")
}

fn sample_state() -> ScoringState {
    ScoringState::new("用户正在调试日志路由。").expect("state 构造")
}

fn adapter_for(base_url: &str) -> JevHttpScoringAdapter {
    JevHttpScoringAdapter::new(base_url, "kev-latest", Duration::from_millis(1500))
}

#[tokio::test]
async fn answer_posts_jev_wire_format_to_systemone_endpoint() {
    let mut server = start_mock_server(MockBehavior::Fixed {
        status: 200,
        body: r#"{"answers": {"q0": {"noul": 0.93}}}"#,
    })
    .await;
    let adapter = adapter_for(&server.base_url);

    let answers = adapter
        .answer(&sample_state(), &[noul_question()])
        .await
        .expect("评分应成功");

    assert!(
        matches!(&answers[0], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.93).abs() < 1e-9)
    );
    let request = server.requests.recv().await.expect("server 应收到请求");
    assert!(
        request.starts_with("POST /v1/systemone "),
        "请求路径应为 /v1/systemone：{}",
        request.lines().next().unwrap_or("")
    );
    assert!(
        request.contains("\"model\":\"kev-latest\""),
        "请求应带 model"
    );
    assert!(request.contains("\"type\":\"noul\""), "请求应带题型");
}

#[tokio::test]
async fn empty_questions_short_circuits_without_http_call() {
    let mut server = start_mock_server(MockBehavior::Fixed {
        status: 200,
        body: r#"{"answers": {}}"#,
    })
    .await;
    let adapter = adapter_for(&server.base_url);

    let answers = adapter
        .answer(&sample_state(), &[])
        .await
        .expect("空批量应成功");

    assert!(answers.is_empty());
    let (idle_sender, idle_receiver) = oneshot::channel::<()>();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = idle_sender.send(());
    });
    tokio::select! {
        received = server.requests.recv() => panic!("空批量不得发起 HTTP 调用：{received:?}"),
        _ = idle_receiver => {}
    }
}

#[tokio::test]
async fn connect_refused_fails_fast_as_connect_unavailable() {
    // 服务未启动时 TCP 预检立即失败（loopback ~100µs），规避 hyper-util 对
    // reusable body 的 connect 重试循环（实测等满总超时才失败）。
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("绑定端口");
    let address = listener.local_addr().expect("本地地址");
    drop(listener);
    let adapter = JevHttpScoringAdapter::new(
        &format!("http://{address}"),
        "kev-latest",
        Duration::from_millis(1500),
    );

    let started = std::time::Instant::now();
    let outcome = adapter.answer(&sample_state(), &[noul_question()]).await;

    let error = outcome.expect_err("连接拒绝必须失败");
    assert_eq!(error.kind(), UnavailableKind::Connect);
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "预检必须快速失败，实际耗时 {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn timeout_maps_to_timeout_unavailable() {
    let server = start_mock_server(MockBehavior::NeverRespond).await;
    let adapter =
        JevHttpScoringAdapter::new(&server.base_url, "kev-latest", Duration::from_millis(100));

    let outcome = adapter.answer(&sample_state(), &[noul_question()]).await;

    let error = outcome.expect_err("超时必须失败");
    assert_eq!(error.kind(), UnavailableKind::Timeout);
}

#[tokio::test]
async fn server_5xx_maps_to_server_unavailable() {
    let server = start_mock_server(MockBehavior::Fixed {
        status: 500,
        body: r#"{"error": "internal"}"#,
    })
    .await;
    let adapter = adapter_for(&server.base_url);

    let outcome = adapter.answer(&sample_state(), &[noul_question()]).await;

    let error = outcome.expect_err("5xx 必须失败");
    assert_eq!(error.kind(), UnavailableKind::Server);
}

#[tokio::test]
async fn schema_422_maps_to_schema_unavailable() {
    let server = start_mock_server(MockBehavior::Fixed {
        status: 422,
        body: r#"{"error": "invalid request"}"#,
    })
    .await;
    let adapter = adapter_for(&server.base_url);

    let outcome = adapter.answer(&sample_state(), &[noul_question()]).await;

    let error = outcome.expect_err("422 必须失败");
    assert_eq!(error.kind(), UnavailableKind::Schema);
}

#[tokio::test]
async fn malformed_response_maps_to_schema_unavailable() {
    let server = start_mock_server(MockBehavior::Fixed {
        status: 200,
        body: "this is not json",
    })
    .await;
    let adapter = adapter_for(&server.base_url);

    let outcome = adapter.answer(&sample_state(), &[noul_question()]).await;

    let error = outcome.expect_err("非法响应必须失败");
    assert_eq!(error.kind(), UnavailableKind::Schema);
}

#[tokio::test]
async fn single_failure_does_not_circuit_break_next_call() {
    let server = start_mock_server(MockBehavior::Sequence(vec![
        (500, r#"{"error": "internal"}"#),
        (200, r#"{"answers": {"q0": {"noul": 0.77}}}"#),
    ]))
    .await;
    let adapter = adapter_for(&server.base_url);

    let first = adapter.answer(&sample_state(), &[noul_question()]).await;
    assert_eq!(
        first.expect_err("首次应 5xx").kind(),
        UnavailableKind::Server
    );

    let second = adapter
        .answer(&sample_state(), &[noul_question()])
        .await
        .expect("失败后下次调用必须正常重试（不熔断）");
    assert!(
        matches!(&second[0], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.77).abs() < 1e-9)
    );
}

#[tokio::test]
async fn batch_questions_round_trip_in_request_order() {
    let mut server = start_mock_server(MockBehavior::Fixed {
        status: 200,
        body: r#"{"answers": {"q1": {"noul": 0.33}, "q0": {"noul": 0.66}}}"#,
    })
    .await;
    let adapter = adapter_for(&server.base_url);
    let questions = vec![noul_question(), noul_question()];

    let answers = adapter
        .answer(&sample_state(), &questions)
        .await
        .expect("批量评分应成功");

    assert_eq!(answers.len(), 2);
    assert!(
        matches!(&answers[0], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.66).abs() < 1e-9)
    );
    assert!(
        matches!(&answers[1], ScoringAnswer::Noul { p_true, .. } if (*p_true - 0.33).abs() < 1e-9)
    );
    let request = server.requests.recv().await.expect("server 应收到请求");
    assert!(request.contains("\"q0\"") && request.contains("\"q1\""));
}

#[test]
fn engine_revision_exposes_configured_model() {
    let adapter = adapter_for("http://127.0.0.1:8009");
    assert_eq!(adapter.engine_revision(), "kev-latest");
}
