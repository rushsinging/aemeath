use super::*;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;

async fn spawn_counting_server(raw_response: &'static str) -> (String, Arc<AtomicUsize>) {
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

#[test]
fn custom_user_agent_is_sent_in_ollama_headers() {
    let provider = OllamaProvider::new_with_user_agent(
        "ollama".to_string(),
        Some("http://localhost:11434".to_string()),
        Some("test-model".to_string()),
        8192,
        false,
        60,
        "aemeath-test/1.0".to_string(),
    );

    assert_eq!(
        provider.build_headers().unwrap().get(USER_AGENT).unwrap(),
        "aemeath-test/1.0"
    );
}

#[tokio::test]
async fn llm_client_ollama_invocation_stream_is_single_request_pull_stream() {
    let body = concat!(
            "{\"message\":{\"role\":\"assistant\",\"content\":\"ol\"},\"done\":false}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"lama\"},\"done\":false}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":1}\n"
        );
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let leaked = Box::leak(response.into_boxed_str());
    let (base_url, requests) = spawn_counting_server(leaked).await;
    let client =
        crate::composition::LlmClient::from_config(crate::composition::LlmConfigOptionsData {
            driver: crate::ProviderDriverKind::Ollama.as_str().to_string(),
            source_key: "ollama".to_string(),
            api_style: None,
            api_key: "ollama".to_string(),
            base_url: Some(base_url),
            model: "test-model".to_string(),
            max_tokens: 8192,
            reasoning: false,
            reasoning_config: None,
            timeout_secs: 60,
            user_agent: Some("aemeath-test/1.0".to_string()),
        })
        .expect("valid ollama config");
    let resolved = crate::ports::ResolvedInvocation::new(
        "test-model",
        8192,
        crate::domain::capability::ReasoningLevel::Off,
        crate::domain::capability::ReasoningLevel::Off,
    )
    .unwrap();

    let events: Vec<_> = client
        .invocation_stream(
            &resolved,
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
            crate::ProviderResponseChunk::Content(crate::ProviderContentData::Text(first)),
            crate::ProviderResponseChunk::Content(crate::ProviderContentData::Text(second)),
            crate::ProviderResponseChunk::Usage(_),
            crate::ProviderResponseChunk::Stop(_)
        ] if first == "ol" && second == "lama"
    ));
    assert_eq!(events.iter().filter(|event| event.is_terminal()).count(), 1);
    let usage = events
        .iter()
        .find_map(|event| match event {
            crate::ProviderResponseChunk::Usage(usage) => Some(usage),
            _ => None,
        })
        .expect("ollama usage reported");
    assert_eq!(usage.input_tokens, Some(1));
    assert_eq!(usage.output_tokens, Some(1));
}
