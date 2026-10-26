//! #2104：tool call arguments 无法解析时的失败载荷契约测试。
//!
//! 七牛等网关偶发把分片拼接成重复前缀（`{"command{"command": ...`，Syntax 类
//! 非 EOF 错误）或全程只发 name 不发 arguments。既有实现把这两种形态静默降级为
//! `{}` 提交执行，导致 LLM 误判「missing required parameter」或空对象真实执行。
//! 契约：流正常结束但参数不可解析 → 发 `ToolCallArgumentsUnparseable` 失败载荷
//! （携带原始文本与诊断），NEVER 再发携带 `{}` 的 `ToolCallCompleted`。

use crate::composition::{LlmClient, ProviderClientSpecData};
use crate::ports::ResolvedInvocation;
use crate::{ProviderContentData, ProviderResponseChunk};
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
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buffer = [0_u8; 8192];
            let _ = socket.read(&mut buffer).await;
            observed.fetch_add(1, Ordering::SeqCst);
            let _ = socket.write_all(raw_response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (format!("http://{addr}"), counter)
}

fn chat_client(base_url: String) -> LlmClient {
    LlmClient::from_config(ProviderClientSpecData {
        driver: crate::domain::capability::ProviderDriverKind::OpenAI
            .as_str()
            .to_string(),
        source_key: "openai".to_string(),
        api_style: None,
        api_key: "test-key".to_string(),
        base_url: Some(base_url),
        model: "test-model".to_string(),
        max_tokens: 8192,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: 60,
        user_agent: Some("aemeath-test/1.0".to_string()),
    })
    .expect("valid OpenAI chat config")
}

fn resolved_scope() -> ResolvedInvocation {
    ResolvedInvocation::new(
        "test-model",
        8192,
        crate::domain::capability::ReasoningLevel::Off,
        crate::domain::capability::ReasoningLevel::Off,
    )
    .unwrap()
}

async fn collect_events(body: &'static str) -> Vec<ProviderResponseChunk> {
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let leaked = Box::leak(response.into_boxed_str());
    let (base_url, _requests) = spawn_openai_counting_server(leaked).await;
    let client = chat_client(base_url);
    futures_util::StreamExt::collect(
        client
            .invocation_stream(
                &resolved_scope(),
                "",
                0,
                &[Message::user("run the tool")],
                &[],
                &CancellationToken::new(),
            )
            .await
            .expect("stream must start"),
    )
    .await
}

/// 七牛实测形态：首帧带 id/name，后续空串 id/name 只带 arguments 分片，
/// 拼接结果出现重复前缀 `{"command{"command": ...`（Syntax 类、非 EOF）。
#[tokio::test]
async fn chat_stream_duplicate_prefix_args_emit_unparseable_payload() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_00_dup\",\"type\":\"function\",\"function\":{\"name\":\"Bash\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"function\":{\"name\":\"\",\"arguments\":\"{\\\"command\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"function\":{\"name\":\"\",\"arguments\":\"{\\\"command\\\": \\\"ls\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let events = collect_events(body).await;

    let unparseable = events.iter().find_map(|event| match event {
        ProviderResponseChunk::Content(ProviderContentData::ToolCallArgumentsUnparseable {
            id,
            name,
            raw_arguments,
            ..
        }) => Some((id.clone(), name.clone(), raw_arguments.clone())),
        _ => None,
    });
    let (payload_id, payload_name, raw_arguments) =
        unparseable.expect("syntax-broken arguments must surface as unparseable payload");
    assert_eq!(payload_id, "call_00_dup");
    assert_eq!(payload_name, "Bash");
    assert_eq!(raw_arguments, "{\"command{\"command\": \"ls\"}");

    // 契约：失败载荷之外 NEVER 出现携带 `{}` 的完成帧（那是静默降级）。
    for event in &events {
        if let ProviderResponseChunk::Content(ProviderContentData::ToolCallCompleted {
            id, ..
        }) = event
        {
            assert_ne!(
                id, "call_00_dup",
                "unparseable tool call must not be completed as executable"
            );
        }
    }
    // 流本身正常终止（非 EOF 截断不触发整轮 StreamTruncated 重试）。
    assert_eq!(
        events.iter().filter(|event| event.is_terminal()).count(),
        1,
        "stream must terminate normally exactly once"
    );
}

/// 模型只发 name、全程无 arguments 分片（ArgumentsMissing 形态）。
#[tokio::test]
async fn chat_stream_name_only_tool_call_emits_unparseable_payload() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_00_nameonly\",\"type\":\"function\",\"function\":{\"name\":\"Bash\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let events = collect_events(body).await;

    let unparseable = events.iter().any(|event| {
        matches!(
            event,
            ProviderResponseChunk::Content(ProviderContentData::ToolCallArgumentsUnparseable {
                id,
                raw_arguments,
                ..
            }) if id == "call_00_nameonly" && raw_arguments.is_empty()
        )
    });
    assert!(
        unparseable,
        "name-only tool call must surface as unparseable payload"
    );
}

/// 回归：合法 JSON 参数的完成帧行为不变。
#[tokio::test]
async fn chat_stream_valid_arguments_still_complete_normally() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_00_ok\",\"type\":\"function\",\"function\":{\"name\":\"Bash\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"function\":{\"name\":\"\",\"arguments\":\"{\\\"command\\\": \\\"ls\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let events = collect_events(body).await;

    let completed = events.iter().any(|event| {
        matches!(
            event,
            ProviderResponseChunk::Content(ProviderContentData::ToolCallCompleted { id, .. })
                if id == "call_00_ok"
        )
    });
    assert!(completed, "valid arguments must complete normally");
    let has_unparseable = events.iter().any(|event| {
        matches!(
            event,
            ProviderResponseChunk::Content(
                ProviderContentData::ToolCallArgumentsUnparseable { .. }
            )
        )
    });
    assert!(!has_unparseable, "valid arguments never emit payload");
}
