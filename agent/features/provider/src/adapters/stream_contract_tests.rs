use super::{invocation_stream_from_decoder, InvocationDecoder};
use crate::{InvocationEvent, ProviderErrorKind, ReasoningLevel};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

async fn response_from_fixture(
    body: &'static str,
    content_type: &'static str,
) -> reqwest::Response {
    response_from_bytes_fixture(body.as_bytes(), content_type).await
}

async fn response_from_bytes_fixture(
    body: &'static [u8],
    content_type: &'static str,
) -> reqwest::Response {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stream contract fixture");
    let address = listener.local_addr().expect("fixture address");
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept fixture request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n\r\n",
            body.len(),
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write fixture response headers");
        socket
            .write_all(body)
            .await
            .expect("write fixture response body");
    });
    reqwest::get(format!("http://{address}/stream"))
        .await
        .expect("read fixture response")
}

async fn assert_success_contract(
    decoder: InvocationDecoder,
    body: &'static str,
    content_type: &'static str,
    expected_text: &str,
) {
    let response = response_from_fixture(body, content_type).await;
    let mut stream = invocation_stream_from_decoder(
        response,
        ReasoningLevel::Off,
        CancellationToken::new(),
        decoder,
    );
    let mut text = String::new();
    let mut terminal_count = 0;
    while let Some(event) = stream.next().await {
        match event {
            InvocationEvent::Delta(crate::InvocationDelta::Text(delta)) => text.push_str(&delta),
            InvocationEvent::Completed(_) => terminal_count += 1,
            InvocationEvent::Failed(error) => panic!("successful fixture failed: {error:?}"),
            InvocationEvent::Delta(_) => {}
        }
    }
    assert_eq!(text, expected_text, "decoder must preserve wire order");
    assert_eq!(
        terminal_count, 1,
        "decoder must emit exactly one terminal event"
    );
    assert!(
        stream.next().await.is_none(),
        "terminal event must end the stream"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_decoder_success_stream_preserves_order_and_ends_after_one_terminal() {
    let cases = [
        (
            InvocationDecoder::Anthropic,
            concat!(
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"an\"}}\n\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"thropic\"}}\n\n",
                "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "data: {\"type\":\"message_stop\"}\n\n"
            ),
            "text/event-stream",
            "anthropic",
        ),
        (
            InvocationDecoder::OpenAiChat,
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"open\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"ai\"},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            ),
            "text/event-stream",
            "openai",
        ),
        (
            InvocationDecoder::OpenAiResponses,
            concat!(
                "event: response.output_text.delta\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"responses\"}\n\n",
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n"
            ),
            "text/event-stream",
            "responses",
        ),
        (
            InvocationDecoder::Ollama,
            concat!(
                "{\"message\":{\"role\":\"assistant\",\"content\":\"ol\"},\"done\":false}\n",
                "{\"message\":{\"role\":\"assistant\",\"content\":\"lama\"},\"done\":false}\n",
                "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\"}\n"
            ),
            "application/x-ndjson",
            "ollama",
        ),
    ];

    for (decoder, body, content_type, expected_text) in cases {
        assert_success_contract(decoder, body, content_type, expected_text).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openai_chunked_body_eof_emits_retryable_stream_interrupted_failure() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind interrupted stream fixture");
    let address = listener.local_addr().expect("fixture address");
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept fixture request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\nA\r\ndata: {}\n\n\r\nF",
            )
            .await
            .expect("write response ending during a chunk-size line");
        // The first chunk is complete. The trailing `F` starts the next chunk-size
        // line, and dropping the socket reproduces an unexpected EOF in that line.
    });

    let response = reqwest::get(format!("http://{address}/stream"))
        .await
        .expect("receive response headers before body interruption");
    let events: Vec<_> = invocation_stream_from_decoder(
        response,
        ReasoningLevel::Off,
        CancellationToken::new(),
        InvocationDecoder::OpenAiChat,
    )
    .collect()
    .await;

    let [InvocationEvent::Failed(error)] = events.as_slice() else {
        panic!("chunk-size EOF must emit exactly one failed terminal event: {events:?}");
    };
    assert_eq!(error.kind, ProviderErrorKind::StreamTruncated);
    assert!(error.retryable);
    let message = error.safe_message.to_ascii_lowercase();
    assert!(
        message.contains("connection interrupted"),
        "failure must preserve the interrupted-connection context: {message}"
    );
    assert!(
        message.contains("eof") || message.contains("chunk"),
        "failure must preserve the chunked EOF cause: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openai_complete_malformed_body_emits_fatal_protocol_failure() {
    let response = response_from_bytes_fixture(
        b"data: {\"choices\":[]}\n\ndata: \xff\n\n",
        "text/event-stream",
    )
    .await;
    let events: Vec<_> = invocation_stream_from_decoder(
        response,
        ReasoningLevel::Off,
        CancellationToken::new(),
        InvocationDecoder::OpenAiChat,
    )
    .collect()
    .await;

    let [InvocationEvent::Failed(error)] = events.as_slice() else {
        panic!("malformed complete body must emit exactly one failed terminal event: {events:?}");
    };
    assert_eq!(error.kind, ProviderErrorKind::Protocol);
    assert!(!error.retryable);
}

/// #1581：不同 call_id 复用同一 `output_index`（SSE 重放 / 网关异常）时，
/// 静默覆盖会丢失前一个 tool_use 而保留其旁路执行的 tool_result，产出孤儿
/// 配对被 Responses API 以 400 拒绝。必须 fail-fast 为可重试的流中断错误。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_duplicate_output_index_fails_fast_as_retryable_interruption() {
    let body = concat!(
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_first\",\"name\":\"Read\"}}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":0,\"arguments\":\"{}\"}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_second\",\"name\":\"Read\"}}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":0,\"arguments\":\"{}\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n"
    );
    let response = response_from_fixture(body, "text/event-stream").await;
    let events: Vec<_> = invocation_stream_from_decoder(
        response,
        ReasoningLevel::Off,
        CancellationToken::new(),
        InvocationDecoder::OpenAiResponses,
    )
    .collect()
    .await;

    let Some(InvocationEvent::Failed(error)) = events.last() else {
        panic!("duplicate output_index must fail fast instead of completing: {events:?}");
    };
    assert!(error.retryable, "must be retryable: {error:?}");
    assert_eq!(error.kind, ProviderErrorKind::StreamTruncated);
    let message = error.safe_message.to_ascii_lowercase();
    assert!(
        message.contains("output_index"),
        "failure must name the duplicated output_index: {message}"
    );
}

/// 同一 call_id 重复 `output_item.added`（幂等重放）不构成协议违规，正常完成。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn responses_repeated_added_for_same_call_id_is_idempotent() {
    let body = concat!(
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_same\",\"name\":\"Read\"}}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":0,\"arguments\":\"{}\"}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_same\",\"name\":\"Read\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n"
    );
    let response = response_from_fixture(body, "text/event-stream").await;
    let events: Vec<_> = invocation_stream_from_decoder(
        response,
        ReasoningLevel::Off,
        CancellationToken::new(),
        InvocationDecoder::OpenAiResponses,
    )
    .collect()
    .await;

    assert!(
        events
            .iter()
            .all(|event| !matches!(event, InvocationEvent::Failed(_))),
        "idempotent replay of the same call_id must not fail: {events:?}"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            InvocationEvent::Completed(completion)
                if completion.stop_reason == crate::published_language::StopReason::ToolUse
        )),
        "the single function_call must still produce a ToolUse completion: {events:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_during_stream_emits_failed_cancelled_then_ends() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind cancellation fixture");
    let address = listener.local_addr().expect("fixture address");
    let first_delta_sent = Arc::new(Notify::new());
    let fixture_signal = first_delta_sent.clone();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept fixture request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n")
            .await
            .expect("write first delta");
        fixture_signal.notify_one();
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    });
    let response = reqwest::get(format!("http://{address}/stream"))
        .await
        .expect("read cancellation fixture");
    let cancel = CancellationToken::new();
    let mut stream = invocation_stream_from_decoder(
        response,
        ReasoningLevel::Off,
        cancel.clone(),
        InvocationDecoder::Anthropic,
    );

    first_delta_sent.notified().await;
    assert!(matches!(
        stream.next().await,
        Some(InvocationEvent::Delta(_))
    ));
    cancel.cancel();
    let terminal = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
        .await
        .expect("cancelled stream must terminate promptly")
        .expect("cancelled stream must expose a terminal event");
    assert!(
        matches!(terminal, InvocationEvent::Failed(ref error) if error.kind == ProviderErrorKind::Cancelled && !error.retryable)
    );
    assert!(stream.next().await.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_consumer_cancels_the_invocation_local_producer() {
    let body = concat!(
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"first\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"second\"}}\n\n"
    );
    let response = response_from_fixture(body, "text/event-stream").await;
    let cancel = CancellationToken::new();
    let stream = invocation_stream_from_decoder(
        response,
        ReasoningLevel::Off,
        cancel.clone(),
        InvocationDecoder::Anthropic,
    );

    drop(stream);
    tokio::time::timeout(std::time::Duration::from_secs(1), cancel.cancelled())
        .await
        .expect("dropping receiver must cancel the producer instead of buffering indefinitely");
}

/// #1494：OpenAI 兼容流——index 切换时对上一个完整 index 发出 ToolCallCompleted，
/// 流结束兜底补发最后一个；不重复发出。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openai_compat_stream_emits_tool_call_completed_on_index_switch_and_stream_end() {
    let body = concat!(
        // index=0 参数分两段累积
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"/tmp\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"/a.txt\\\"}\"}}]}}]}\n\n",
        // index=1 出现 → index=0 参数已完整，应在此处发出 ToolCallCompleted
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_1\",\"function\":{\"name\":\"write_file\",\"arguments\":\"{\\\"text\\\":\\\"hi\\\"}\"}}]}}]}\n\n",
        // 流结束（finish_reason=tool_calls + [DONE]）→ index=1 兜底发出
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let response = response_from_fixture(body, "text/event-stream").await;
    let mut stream = invocation_stream_from_decoder(
        response,
        ReasoningLevel::Off,
        CancellationToken::new(),
        InvocationDecoder::OpenAiChat,
    );

    let mut completed: Vec<(usize, String)> = Vec::new();
    let mut completed_positions: Vec<usize> = Vec::new();
    let mut started: Vec<usize> = Vec::new();
    let mut event_index = 0;
    while let Some(event) = stream.next().await {
        match event {
            InvocationEvent::Delta(crate::InvocationDelta::ToolCallStarted {
                index, name, ..
            }) => {
                started.push(index);
                let _ = name;
            }
            InvocationEvent::Delta(crate::InvocationDelta::ToolCallCompleted { index, call }) => {
                completed.push((index, call.name));
                completed_positions.push(event_index);
            }
            InvocationEvent::Completed(_) => {}
            InvocationEvent::Failed(error) => panic!("fixture failed: {error:?}"),
            InvocationEvent::Delta(_) => {}
        }
        event_index += 1;
    }

    // 两个 tool call 都发出且不重复。
    assert_eq!(
        completed,
        vec![(0, "read_file".to_string()), (1, "write_file".to_string())],
        "ToolCallCompleted must be emitted once per tool call, in index order"
    );
    assert_eq!(started, vec![0, 1], "ToolCallStarted order preserved");
    // index=0 的 completed 出现在 index=1 的 started 之后（切换时发出）、流结束前。
    let completed_0 = completed_positions[0];
    let started_1 = started.iter().position(|i| *i == 1).unwrap();
    assert!(
        completed_0 > started_1,
        "index=0 ToolCallCompleted must be emitted when index=1 first appears (before stream end), got completed@{completed_0} started_1@{started_1}"
    );
}

/// #1494：Anthropic 流——ContentBlockStop 即发出 ToolCallCompleted（协议级完整信号）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anthropic_stream_emits_tool_call_completed_on_content_block_stop() {
    let body = concat!(
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_file\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"/tmp\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"/a.txt\\\"}\"}}\n\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let response = response_from_fixture(body, "text/event-stream").await;
    let mut stream = invocation_stream_from_decoder(
        response,
        ReasoningLevel::Off,
        CancellationToken::new(),
        InvocationDecoder::Anthropic,
    );

    let mut completed: Vec<(usize, String, serde_json::Value)> = Vec::new();
    while let Some(event) = stream.next().await {
        match event {
            InvocationEvent::Delta(crate::InvocationDelta::ToolCallCompleted { index, call }) => {
                completed.push((index, call.name, call.arguments));
            }
            InvocationEvent::Failed(error) => panic!("fixture failed: {error:?}"),
            _ => {}
        }
    }

    assert_eq!(completed.len(), 1, "exactly one ToolCallCompleted");
    assert_eq!(completed[0].0, 0);
    assert_eq!(completed[0].1, "read_file");
    assert_eq!(
        completed[0].2,
        serde_json::json!({"path": "/tmp/a.txt"}),
        "arguments must be the validated JSON"
    );
}
