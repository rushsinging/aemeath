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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stream contract fixture");
    let address = listener.local_addr().expect("fixture address");
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept fixture request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write fixture response");
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
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n20\r\ndata: {\"choices\":[{\"delta\":{\r\n",
            )
            .await
            .expect("write partial chunked response");
        // The advertised chunk is deliberately incomplete. Dropping the socket
        // reproduces a proxy/upstream connection cut during an SSE body read.
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

    assert!(matches!(
        events.as_slice(),
        [InvocationEvent::Failed(error)]
            if error.kind == ProviderErrorKind::StreamTruncated && error.retryable
    ));
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
