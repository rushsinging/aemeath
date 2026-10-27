//! #2104：流中 arguments 无法解析的 tool call 短路契约测试。
//!
//! 契约：reducer 收到 `ToolCallArgumentsUnparseable` 失败帧 →
//! 1. 聚合 output 落一条配对骨架 `ToolCall { input: {} }`（assistant 消息侧
//!    tool_use/tool_result 配对不变式）；
//! 2. NEVER 调 `submit`（不执行），改走 `record_unparseable` 物化 error 结果；
//! 3. `StreamingToolExecutor::record_unparseable` 产出的 round 携带
//!    `is_error` ToolExecution，text 含诊断与原始片段前 500 字符，
//!    经 `take_results` 供 engine Tools 阶段统一汇总回给 LLM。

use super::stream_handler::InvocationEventReducer;
use super::streaming_tool::StreamingToolSubmitPort;
use crate::application::loop_engine::chat::events::EventFuture;
use crate::application::loop_engine::chat::RuntimeStreamEvent;
use crate::application::tool::agent::ToolCall;
use provider::{ProviderContentData, ProviderResponseChunk};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct RecordingSink(Arc<Mutex<Vec<RuntimeStreamEvent>>>);

impl crate::application::loop_engine::chat::ChatEventSink for RecordingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(async move { self.0.lock().unwrap().push(event) })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.0.lock().unwrap().push(event);
    }
}

/// 记录型提交端口：区分 submit（执行）与 record_unparseable（短路）。
#[derive(Clone, Default)]
struct RecordingSubmitPort {
    submitted: Arc<Mutex<Vec<ToolCall>>>,
    unparseable: Arc<Mutex<Vec<(ToolCall, String, String)>>>,
}

impl StreamingToolSubmitPort for RecordingSubmitPort {
    fn submit(&self, call: ToolCall) {
        self.submitted.lock().unwrap().push(call);
    }

    fn record_unparseable(&self, call: ToolCall, raw_arguments: String, diagnostic: String) {
        self.unparseable
            .lock()
            .unwrap()
            .push((call, raw_arguments, diagnostic));
    }
}

fn unparseable_frame() -> ProviderContentData {
    ProviderContentData::ToolCallArgumentsUnparseable {
        index: 0,
        id: "call_00_dup".to_string(),
        name: "Bash".to_string(),
        raw_arguments: "{\"command{\"command\": \"ls\"}".to_string(),
        diagnostic: "expected `:` at line 1 column 12".to_string(),
    }
}

#[test]
fn reducer_shortcuts_unparseable_frame_to_record_without_submit() {
    let sink = RecordingSink::default();
    let port = RecordingSubmitPort::default();
    let submitted = port.submitted.clone();
    let unparseable = port.unparseable.clone();
    let mut reducer = InvocationEventReducer::new(sink).with_streaming_tool(Arc::new(port));

    assert!(reducer
        .apply(ProviderResponseChunk::Content(unparseable_frame()))
        .unwrap()
        .is_none());
    let response = reducer
        .apply(ProviderResponseChunk::Stop(
            provider::ResponseStopReason::ToolUse,
        ))
        .unwrap()
        .expect("stop frame produces response");

    // 1. 配对骨架：assistant 消息含 ToolUse 且 input 为空对象。
    let tool_use_blocks: Vec<_> = response
        .assistant_message
        .content
        .iter()
        .filter(|block| matches!(block, share::message::ContentBlock::ToolUse { .. }))
        .collect();
    assert_eq!(tool_use_blocks.len(), 1, "exactly one skeleton ToolUse");
    assert!(matches!(
        tool_use_blocks[0],
        share::message::ContentBlock::ToolUse { id, input, .. }
            if id == "call_00_dup" && input == &serde_json::json!({})
    ));

    // 2. NEVER submit 执行；record_unparseable 恰好一次且携带原文。
    assert!(
        submitted.lock().unwrap().is_empty(),
        "unparseable tool call MUST NOT be submitted for execution"
    );
    let records = unparseable.lock().unwrap().clone();
    assert_eq!(records.len(), 1);
    let (call, raw_arguments, diagnostic) = &records[0];
    assert_eq!(call.provider_id, "call_00_dup");
    assert_eq!(call.name, "Bash");
    assert_eq!(raw_arguments, "{\"command{\"command\": \"ls\"}");
    assert!(diagnostic.contains("expected"));
}

#[test]
fn reducer_marks_unparseable_frame_as_visible_delta() {
    // 只有失败帧（无文本）的流也不能被 empty_completion_error 判空：
    // 骨架 ToolCall 是 actionable output。
    let sink = RecordingSink::default();
    let mut reducer = InvocationEventReducer::new(sink)
        .with_streaming_tool(Arc::new(RecordingSubmitPort::default()));

    assert!(reducer
        .apply(ProviderResponseChunk::Content(unparseable_frame()))
        .unwrap()
        .is_none());
    let response = reducer
        .apply(ProviderResponseChunk::Stop(
            provider::ResponseStopReason::ToolUse,
        ))
        .unwrap()
        .expect("skeleton ToolCall keeps the stream actionable");
    assert_eq!(response.stop_reason, provider::ResponseStopReason::ToolUse);
}

#[test]
fn unparseable_tool_execution_carries_diagnostic_and_raw_preview() {
    let call = ToolCall {
        id: sdk::ids::ToolCallId::from_legacy_or_new("call_00_dup"),
        provider_id: "call_00_dup".to_string(),
        name: "Bash".to_string(),
        index: 0,
        input: serde_json::json!({}),
    };
    let execution = super::streaming_tool::unparseable_tool_execution(
        &call,
        "{\"command{\"command\": \"ls\"}",
        "expected `:` at line 1 column 12",
    );
    assert!(execution.outcome.is_error);
    assert!(execution
        .outcome
        .text
        .contains("could not be parsed as JSON"));
    assert!(execution
        .outcome
        .text
        .contains("{\"command{\"command\": \"ls\"}"));
    assert_eq!(execution.call_id, call.id);
    assert_eq!(execution.provider_id, "call_00_dup");
    assert_eq!(execution.tool_name, "Bash");
}
