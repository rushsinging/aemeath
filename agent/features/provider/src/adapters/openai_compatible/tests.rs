use futures_util::StreamExt;
use share::message::Message;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

async fn spawn_openai_counting_server(raw_response: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let counter = Arc::new(AtomicUsize::new(0));
    let observed = counter.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            observed.fetch_add(1, Ordering::SeqCst);
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buffer = [0_u8; 8192];
            let _ = socket.read(&mut buffer).await;
            let _ = socket.write_all(raw_response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (format!("http://{addr}"), counter)
}

#[tokio::test]
async fn llm_client_chat_invocation_stream_is_single_request_pull_stream() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"open\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"ai\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let leaked = Box::leak(response.into_boxed_str());
    let (base_url, requests) = spawn_openai_counting_server(leaked).await;
    let client = crate::LlmClient::from_config(crate::LlmConfigOptions {
        driver: crate::ProviderDriverKind::OpenAI.as_str().to_string(),
        source_key: "openai".to_string(),
        api_style: None,
        api_key: "test-key".to_string(),
        base_url: Some(base_url),
        model: "test-model".to_string(),
        max_tokens: 8192,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: 60,
    })
    .expect("valid OpenAI chat config");
    let scope = crate::InvocationScope::new(
        "test-model",
        8192,
        crate::ReasoningLevel::Off,
        crate::ReasoningLevel::Off,
    )
    .unwrap();

    let events: Vec<_> = client
        .invocation_stream(
            &scope,
            &[],
            &[Message::user("hi")],
            &[],
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .collect()
        .await;

    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(matches!(
        &events[..],
        [
            crate::InvocationEvent::Delta(crate::InvocationDelta::Text(first)),
            crate::InvocationEvent::Delta(crate::InvocationDelta::Text(second)),
            crate::InvocationEvent::Completed(_)
        ] if first == "open" && second == "ai"
    ));
    assert_eq!(events.iter().filter(|event| event.is_terminal()).count(), 1);
}

#[tokio::test]
async fn llm_client_responses_invocation_stream_is_single_request_pull_stream() {
    let body = concat!(
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"response\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let leaked = Box::leak(response.into_boxed_str());
    let (base_url, requests) = spawn_openai_counting_server(leaked).await;
    let client = crate::LlmClient::from_config(crate::LlmConfigOptions {
        driver: crate::ProviderDriverKind::OpenAI.as_str().to_string(),
        source_key: "openai".to_string(),
        api_style: Some("responses".to_string()),
        api_key: "test-key".to_string(),
        base_url: Some(base_url),
        model: "test-model".to_string(),
        max_tokens: 8192,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: 60,
    })
    .expect("valid OpenAI responses config");
    let scope = crate::InvocationScope::new(
        "test-model",
        8192,
        crate::ReasoningLevel::Off,
        crate::ReasoningLevel::Off,
    )
    .unwrap();

    let events: Vec<_> = client
        .invocation_stream(
            &scope,
            &[],
            &[Message::user("hi")],
            &[],
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .collect()
        .await;

    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(matches!(
        &events[..],
        [
            crate::InvocationEvent::Delta(crate::InvocationDelta::Text(text)),
            crate::InvocationEvent::Completed(_)
        ] if text == "response"
    ));
    assert_eq!(events.iter().filter(|event| event.is_terminal()).count(), 1);
}

include!("tests/common.rs");
include!("tests/reasoning.rs");
include!("tests/provider_config.rs");
include!("tests/clamp_effort.rs");

#[test]
fn chat_usage_prefers_reported_total_without_double_counting_cache() {
    let usage = super::usage::parse_chat_usage(&serde_json::json!({
        "prompt_tokens": 100,
        "completion_tokens": 20,
        "total_tokens": 150,
        "prompt_tokens_details": {"cached_tokens": 80},
        "completion_tokens_details": {"reasoning_tokens": 5}
    }));

    assert_eq!(usage.cached_tokens, Some(80));
    assert_eq!(usage.reasoning_tokens, Some(5));
    assert_eq!(usage.total_tokens, Some(150));
}

#[test]
fn responses_usage_falls_back_to_input_plus_output_when_total_missing() {
    let usage = super::usage::parse_responses_usage(&serde_json::json!({
        "input_tokens": 100,
        "output_tokens": 20,
        "input_tokens_details": {"cached_tokens": 80},
        "output_tokens_details": {"reasoning_tokens": 5}
    }));

    assert_eq!(usage.cached_tokens, Some(80));
    assert_eq!(usage.reasoning_tokens, Some(5));
    assert_eq!(usage.total_tokens, Some(120));
}
