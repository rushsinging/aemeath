use super::{bridge_context_observations, invocation_stream_from_decoder, InvocationDecoder};
use crate::adapters::wire::StreamEvent;
use crate::domain::capability::ReasoningLevel;
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_stream_bridge_preserves_each_callers_opaque_log_context() {
    bridge_context_observations()
        .lock()
        .expect("bridge context observations lock poisoned")
        .clear();

    let server = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture server");
    let address = server.local_addr().expect("fixture server address");
    let fixture = concat!(
        "HTTP/1.1 200 OK\r\n",
        "content-type: text/event-stream\r\n",
        "connection: close\r\n\r\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,",
        "\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n",
        "data: [DONE]\n\n"
    );
    let fixture_server = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = server.accept().await.expect("accept fixture request");
            let mut request = [0_u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut request).await;
            tokio::io::AsyncWriteExt::write_all(&mut socket, fixture.as_bytes())
                .await
                .expect("write fixture response");
        }
    });

    let run = |request_id: &'static str| async move {
        let context = logging::LogContext {
            session_id: Some(format!("session-{request_id}")),
            request_id: Some(request_id.to_string()),
            ..logging::LogContext::default()
        };
        logging::instrument(context.clone(), async move {
            let response = reqwest::get(format!("http://{address}/{request_id}"))
                .await
                .expect("fixture response");
            let mut stream = invocation_stream_from_decoder(
                response,
                ReasoningLevel::Off,
                CancellationToken::new(),
                InvocationDecoder::Anthropic,
            );
            while stream.next().await.is_some() {}
            context
        })
        .await
    };

    let (first, second) = tokio::join!(run("request-a"), run("request-b"));
    fixture_server.await.expect("fixture server task");

    let observations = bridge_context_observations()
        .lock()
        .expect("bridge context observations lock poisoned")
        .clone();
    for expected in [first, second] {
        for stage in ["producer", "event", "consumer"] {
            assert!(
                observations
                    .iter()
                    .any(|(observed_stage, context)| *observed_stage == stage
                        && context == &expected),
                "missing {stage} observation for {expected:?}; got {observations:?}"
            );
        }
    }
}

#[test]
fn anthropic_message_start_deserializes_all_input_token_components() {
    let event: StreamEvent = serde_json::from_value(serde_json::json!({
        "type": "message_start",
        "message": {
            "usage": {
                "input_tokens": 100,
                "cache_read_input_tokens": 80,
                "cache_creation_input_tokens": 30,
                "output_tokens": 0
            }
        }
    }))
    .expect("valid Anthropic message_start fixture");

    let StreamEvent::MessageStart { message } = event else {
        panic!("expected message_start");
    };
    assert_eq!(message.usage.input_tokens, 100);
    assert_eq!(message.usage.cached_tokens, Some(80));
    assert_eq!(message.usage.cache_creation_tokens, Some(30));
    assert_eq!(message.usage.normalized_total_tokens(110), 210);
}
