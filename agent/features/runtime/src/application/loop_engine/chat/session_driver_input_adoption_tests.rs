/// freeze 时 LoopInput 携带 images → 生成含 image content block 的 Message，
/// 而非纯文本 Message（不丢 images 语义）。
#[test]
fn freeze_preserves_images_from_loop_input() {
    let img = sdk::ChatInputImage {
        id: "[Image #1]".to_string(),
        base64: "aW1n".to_string(),
        media_type: "image/png".to_string(),
    };
    let input = crate::application::loop_engine::LoopInput {
        text: "看图[Image #1]".to_string(),
        input_id: Some(sdk::InputId::new_v7()),
        images: vec![img],
        accepted: None,
    };
    let (frozen, _active) = fixture_bind_pending(Vec::new(), &[input]);

    assert_eq!(frozen.len(), 1);
    // text_content 还原含占位符的完整文本
    assert_eq!(frozen[0].text_content(), "看图[Image #1]");
    // content 中应含 Image block（非纯文本）
    let has_image = frozen[0]
        .content
        .iter()
        .any(|b| matches!(b, share::message::ContentBlock::Image { .. }));
    assert!(
        has_image,
        "freeze must produce Image content block when LoopInput carries images"
    );
}

#[test]
fn freeze_preserves_skill_request_metadata_from_typed_loop_input() {
    let input_id = sdk::InputId::new_v7();
    let request = sdk::SkillRequest {
        input_id: input_id.clone(),
        skill: "superpowers:brainstorming".to_string(),
        arguments: "feature scope".to_string(),
        raw_input: "/superpowers:brainstorming feature scope".to_string(),
    };
    let input = crate::application::loop_engine::LoopInput::accepted(
        crate::application::loop_engine::AcceptedUserInput::SkillRequest(request.clone()),
    );
    let (frozen, _) = fixture_bind_pending(Vec::new(), &[input]);

    assert_eq!(frozen.len(), 1);
    assert_eq!(frozen[0].source(), share::message::MessageSource::SkillRequest);
    assert!(frozen[0].text_content().contains("<skill-request>"));
    assert!(!frozen[0].text_content().contains(&request.raw_input));
    assert_eq!(
        frozen[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.skill_request.as_ref()),
        Some(&share::message::SkillRequestMetadata {
            skill: request.skill,
            arguments: request.arguments,
            raw_input: request.raw_input,
        })
    );
}

/// freeze 时 LoopInput 无 images → 生成纯文本 Message（与旧行为一致）。
#[test]
fn freeze_text_only_when_no_images() {
    let input = crate::application::loop_engine::LoopInput {
        text: "hello".to_string(),
        input_id: Some(sdk::InputId::new_v7()),
        images: Vec::new(),
        accepted: None,
    };
    let (frozen, _active) = fixture_bind_pending(Vec::new(), &[input]);

    assert_eq!(frozen.len(), 1);
    assert_eq!(frozen[0].text_content(), "hello");
    // 无 images 时应为纯 Text 块
    let all_text = frozen[0]
        .content
        .iter()
        .all(|b| matches!(b, share::message::ContentBlock::Text { .. }));
    assert!(all_text, "text-only input should produce text-only message");
}

/// freeze 时 inputs 为空 → fallback 到 pending（保留 pending 中的 images）。
#[test]
fn freeze_empty_inputs_uses_pending_with_images() {
    let img = sdk::ChatInputImage {
        id: "[Image #1]".to_string(),
        base64: "aW1n".to_string(),
        media_type: "image/png".to_string(),
    };
    let pending_msg = share::message::Message::user_with_images(
        "看图[Image #1]".to_string(),
        vec![(img.id, img.base64, img.media_type)],
    );
    let (frozen, _active) = fixture_bind_pending(vec![pending_msg], &[]);

    assert_eq!(frozen.len(), 1);
    assert_eq!(frozen[0].text_content(), "看图[Image #1]");
    let has_image = frozen[0]
        .content
        .iter()
        .any(|b| matches!(b, share::message::ContentBlock::Image { .. }));
    assert!(
        has_image,
        "pending messages with images must be preserved when inputs are empty"
    );
}

// ── #1272 idle initial input replay regression ──────────────────────────

/// Regression: idle gate-adopted user input must NOT be replayed when a
/// subsequent step has empty inputs (InternalContinuation / ToolResults).
///
/// Before the fix, `loop_runner` passed the same gate-adopted messages to
/// both `RunInputBuffer` (as step input) and the Run execution state's pending input
/// (as pending). On the first step the buffer drain was non-empty, so
/// `freeze` consumed the drained inputs and left `pending` untouched. On
/// the next step with empty inputs (e.g., ToolResults continuation),
/// `freeze` fell through to `pending`, re-introducing the same user
/// messages into `accepted_input` and the LLM context.
///
/// After the fix, the execution state starts with an empty pending input set.
/// The gate-adopted input enters the Run exclusively via `RunInputBuffer`.
#[test]
fn idle_initial_input_not_replayed_on_empty_continuation_step() {
    use super::main_run_port::fixture_two_step_accepted;
    use crate::application::loop_engine::chat::run_input_buffer::{BufferDrain, RunInputBuffer};
    use crate::application::loop_engine::DrainEpoch;

    let input_id = sdk::InputId::new_v7();

    // --- Simulate production flow ---

    // 1. Seed RunInputBuffer with gate-adopted event (as loop_runner does)
    let mut buffer = RunInputBuffer::new();
    buffer.push(sdk::ChatInputEvent::UserMessage {
        id: input_id.clone(),
        text: "initial user message".to_string(),
        images: Vec::new(),
    });

    // 2. First step drain: buffer has the event → Ready
    let first_inputs = match buffer.drain_or_seal(DrainEpoch(0)) {
        BufferDrain::Ready { batch, .. } => batch,
        other => panic!("first drain should be Ready, got {other:?}"),
    };
    assert_eq!(first_inputs.len(), 1);
    assert_eq!(first_inputs[0].text, "initial user message");

    // 3. Second step drain: InternalContinuation (buffer now empty → empty batch)
    let second_inputs = match buffer.take_internal_continuation(DrainEpoch(1)) {
        BufferDrain::Ready { batch, .. } => batch,
        other => panic!("second drain should be Ready (empty), got {other:?}"),
    };
    assert!(
        second_inputs.is_empty(),
        "InternalContinuation batch must be empty"
    );

    // --- RunExecutionState pending-input lifecycle (after fix: empty pending) ---
    let (first_accepted, second_accepted) = fixture_two_step_accepted(
        Vec::new(),     // FIX: empty pending (not messages.clone())
        &first_inputs,  // from buffer drain
        &second_inputs, // empty (InternalContinuation)
    );

    // First step: exactly one accepted user message
    assert_eq!(
        first_accepted.len(),
        1,
        "first step: initial input accepted exactly once"
    );
    assert_eq!(first_accepted[0].text_content(), "initial user message");

    // Second step: NO replayed user messages from stale pending
    assert_eq!(
        second_accepted.len(),
        0,
        "second step must NOT replay the initial input (no stale pending)"
    );

    // UserMessagesAdopted: adopted input is now owned by RunExecutionState and derived from inputs' input_id.
    // First step has input_id → exactly one UserMessagesAdopted.
    // Second step has empty inputs → zero UserMessagesAdopted.
    // This proves exactly-once adoption across both steps.
    let total_adopted = first_inputs
        .iter()
        .chain(second_inputs.iter())
        .filter(|i| i.input_id.is_some())
        .count();
    assert_eq!(
        total_adopted, 1,
        "exactly one UserMessagesAdopted emission across both steps"
    );
}

/// Counter-test: demonstrates the replay bug when `pending` contains the
/// same messages as the buffer (the pre-fix `messages.clone()` behavior).
/// This test documents WHY the fix is necessary — it shows the stale
/// `pending` leaking into the second step's `accepted_input`.
#[test]
fn stale_pending_replays_initial_input_on_empty_continuation_step() {
    use super::main_run_port::fixture_two_step_accepted;
    use crate::application::loop_engine::LoopInput;

    let input = LoopInput {
        text: "initial".to_string(),
        input_id: Some(sdk::InputId::new_v7()),
        images: Vec::new(),
        accepted: None,
    };

    // Pre-fix behavior: pending = messages.clone() (same content as buffer)
    let (first_accepted, second_accepted) = fixture_two_step_accepted(
        vec![Message::user("initial")], // BUGGY: stale pending
        &[input],                       // first step: from buffer
        &[],                            // second step: empty
    );

    assert_eq!(first_accepted.len(), 1, "first step: one accepted");
    // BUG: pending leaks into second step's accepted_input
    assert_eq!(
        second_accepted.len(),
        1,
        "pre-fix bug: stale pending replays into second step"
    );
}

// ─── #1272 per-turn drain seal fixtures ────────────────────────────────
//
// These verify the production drain→freeze→adopt pipeline through
// `run_session_command_driver`. The invariants are:
//   1. `Context::append_accepted_input` is called exactly once for the
//      initial user message (observable as exactly one user occurrence per
//      LLM payload).
//   2. `RuntimeStreamEvent::UserMessagesAdopted` is emitted exactly once
//      per accepted batch with the original `InputId` preserved.
//   3. No duplicate user messages in any single provider request payload.

/// Minimal real typed tool that returns a deterministic marker.
/// Registered through `TestCatalogExecutionFactory` so the engine
/// exercises the real `TypedToolAdapter` → `ToolExecutionPort` path.
struct NoopMarkerTool;

#[derive(serde::Serialize)]
struct NoopMarkerOutput {
    marker: &'static str,
}

#[async_trait]
impl ::tools::TypedTool for NoopMarkerTool {
    type Output = NoopMarkerOutput;
    fn name(&self) -> &str {
        "NoopMarker"
    }
    fn description(&self) -> &str {
        "Test-only noop tool that returns a deterministic marker."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {"marker": {"type": "string"}},
            "required": ["marker"]
        })
    }
    fn data_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {"marker": {"type": "string"}},
            "required": ["marker"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }
    async fn call(
        &self,
        _input: serde_json::Value,
        _ctx: &::tools::ToolExecutionContext,
    ) -> ::tools::TypedToolResult<Self::Output> {
        ::tools::TypedToolResult::success(
            "noop-marker-result",
            NoopMarkerOutput {
                marker: "noop-marker-result",
            },
        )
    }
}

/// LLM provider that returns a real tool call on the first invocation
/// (matching `NoopMarker` registered in `TestCatalogExecutionFactory`)
/// and plain text on the second invocation.
///
/// Notify signals fire after each invocation so the driver can observe
/// boundaries deterministically without sleeps.
struct ToolThenTextProvider {
    call_count: Arc<Mutex<usize>>,
    /// Recorded `request.messages` per invocation for replay detection.
    recorded_messages: Arc<Mutex<Vec<Vec<Message>>>>,
    after_first: Arc<tokio::sync::Notify>,
    after_second: Arc<tokio::sync::Notify>,
}

impl ToolThenTextProvider {
    fn new(after_first: Arc<tokio::sync::Notify>, after_second: Arc<tokio::sync::Notify>) -> Self {
        Self {
            call_count: Arc::new(Mutex::new(0)),
            recorded_messages: Arc::new(Mutex::new(Vec::new())),
            after_first,
            after_second,
        }
    }
}

#[async_trait]
impl LlmProvider for ToolThenTextProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let call_num = {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            *count
        };
        self.recorded_messages
            .lock()
            .unwrap()
            .push(messages.to_vec());

        if call_num == 1 {
            let tool_call = ProviderToolCall {
                id: ProviderToolCallId("toolu_noop_001".to_string()),
                name: "NoopMarker".to_string(),
                arguments: serde_json::json!({"marker": "noop-marker-result"}),
            };
            self.after_first.notify_one();
            Ok(Box::pin(futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::ToolCallStarted {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_noop_001".to_string())),
                    name: "NoopMarker".to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolArgumentsDelta {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_noop_001".to_string())),
                    partial_json: r#"{"marker":"noop-marker-result"}"#.to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolCallCompleted {
                    index: 0,
                    call: tool_call,
                }),
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::ToolCall(ProviderToolCall {
                        id: ProviderToolCallId("toolu_noop_001".to_string()),
                        name: "NoopMarker".to_string(),
                        arguments: serde_json::json!({"marker": "noop-marker-result"}),
                    })],
                    stop_reason: ProviderStopReason::ToolUse,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(10),
                        output_tokens: Some(20),
                        ..RawUsageSnapshot::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ])))
        } else {
            self.after_second.notify_one();
            Ok(Box::pin(futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::Text("finished".to_string())),
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::Text("finished".to_string())],
                    stop_reason: ProviderStopReason::EndTurn,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(15),
                        output_tokens: Some(3),
                        ..RawUsageSnapshot::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ])))
        }
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

/// LLM provider that returns plain text (no tools) on a single invocation.
struct TextOnlyProvider {
    call_count: Arc<Mutex<usize>>,
    recorded_messages: Arc<Mutex<Vec<Vec<Message>>>>,
    after_response: Arc<tokio::sync::Notify>,
}

impl TextOnlyProvider {
    fn new(after_response: Arc<tokio::sync::Notify>) -> Self {
        Self {
            call_count: Arc::new(Mutex::new(0)),
            recorded_messages: Arc::new(Mutex::new(Vec::new())),
            after_response,
        }
    }
}

#[async_trait]
impl LlmProvider for TextOnlyProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
        }
        self.recorded_messages
            .lock()
            .unwrap()
            .push(messages.to_vec());
        self.after_response.notify_one();
        Ok(Box::pin(futures::stream::iter(vec![
            InvocationEvent::Delta(InvocationDelta::Text("hello from model".to_string())),
            InvocationEvent::Completed(ProviderCompletion {
                output: vec![ProviderContentBlock::Text("hello from model".to_string())],
                stop_reason: ProviderStopReason::EndTurn,
                usage: Some(RawUsageSnapshot {
                    input_tokens: Some(5),
                    output_tokens: Some(3),
                    ..RawUsageSnapshot::default()
                }),
                effective_reasoning: ReasoningLevel::Off,
            }),
        ])))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

// ─── #1272 drain seal tests ────────────────────────────────────────────

/// #1272: idle initial user input → gate adopts → provider returns a real
/// NoopMarker tool call → engine processes tool → InternalContinuation →
/// provider returns text → Run terminates.
///
/// Asserts:
///   a) `UserMessagesAdopted` emitted exactly once with the correct InputId.
///   b) The LLM never sees the user message duplicated within a single
///      request payload.
///   c) DoneWithDuration emitted (loop terminated).
///   d) Provider saw exactly 2 invocations.
#[tokio::test]
async fn per_turn_drain_seal_initial_user_message_not_replayed_on_tool_results_continuation() {
    let after_first = Arc::new(tokio::sync::Notify::new());
    let after_second = Arc::new(tokio::sync::Notify::new());

    let provider = Arc::new(ToolThenTextProvider::new(
        after_first.clone(),
        after_second.clone(),
    ));
    let recorded = provider.recorded_messages.clone();
    let call_count = provider.call_count.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    // Build catalog with the real NoopMarker typed tool.
    let factory = ::tools::composition::TestCatalogExecutionFactory::new();
    factory.register(NoopMarkerTool);
    let tool_ctx = crate::application::run::workspace_test_support::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        Default::default(),
    );
    let wired = factory.build(tool_ctx);
    let _catalog_port = wired.catalog_port();
    let _execution = wired.execution();

    let user_input_id = sdk::InputId::new_v7();
    let user_text = "first-message-marker-1272";

    input_tx
        .send(sdk::ChatInputEvent::UserMessage {
            id: user_input_id.clone(),
            text: user_text.to_string(),
            images: Vec::new(),
        })
        .expect("input channel is open");

    let shell = test_shell_with_hooks(test_hook_port());
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-per-turn-drain-seal");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);

    let driver_sink = sink.clone();
    let driver_after_first = after_first.clone();
    let driver_after_second = after_second.clone();
    let driver = tokio::spawn(async move {
        driver_after_first.notified().await;
        driver_after_second.notified().await;
        loop {
            let done = driver_sink.events().iter().any(|e| e == "DoneWithDuration");
            if done {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver completes after ToolResults continuation + final text");

    driver.await.expect("driver joins cleanly");

    // ── Invariant a: UserMessagesAdopted emitted exactly once ──────────
    let adopted = sink.adopted_ids();
    assert_eq!(
        adopted.len(),
        1,
        "UserMessagesAdopted must be emitted exactly once, got {adopted:?}"
    );
    assert_eq!(
        adopted[0],
        vec![user_input_id.as_str().to_string()],
        "InputId must equal the original ChatInputEvent::UserMessage::id"
    );

    // ── Invariant b: LLM did not see duplicate user messages per call ──
    let recorded = recorded.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "expected exactly two model invocations (tool call + final text)"
    );
    for (idx, payload) in recorded.iter().enumerate() {
        let count = payload
            .iter()
            .filter(|m| matches!(m.role, Role::User) && m.text_content() == user_text)
            .count();
        assert!(
            count <= 1,
            "LLM invocation #{idx} saw the user message {count} times (must be ≤ 1)"
        );
    }
    let first_count = recorded[0]
        .iter()
        .filter(|m| matches!(m.role, Role::User) && m.text_content() == user_text)
        .count();
    let second_count = recorded[1]
        .iter()
        .filter(|m| matches!(m.role, Role::User) && m.text_content() == user_text)
        .count();
    assert_eq!(
        first_count, 1,
        "first invocation: user message exactly once"
    );
    assert_eq!(
        second_count, 1,
        "second invocation (post-tool-results): user message exactly once — no double-drain replay"
    );

    // ── Invariant c: DoneWithDuration emitted ──────────────────────────
    assert!(
        sink.events().iter().any(|e| e == "DoneWithDuration"),
        "Run must terminate with DoneWithDuration"
    );

    // ── Invariant d: provider saw exactly 2 invocations ────────────────
    assert_eq!(*call_count.lock().unwrap(), 2, "tool call + final text");
}

/// #1272: Same Main Run service assembly point, verifies the post-tool
/// continuation re-entry does NOT re-emit UserMessagesAdopted for the
/// original user message.
#[tokio::test]
async fn per_turn_drain_seal_input_id_preserved_when_run_returns_tool_results_with_fresh_input() {
    let after_first = Arc::new(tokio::sync::Notify::new());
    let after_second = Arc::new(tokio::sync::Notify::new());

    let provider = Arc::new(ToolThenTextProvider::new(
        after_first.clone(),
        after_second.clone(),
    ));
    let recorded = provider.recorded_messages.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    let factory = ::tools::composition::TestCatalogExecutionFactory::new();
    factory.register(NoopMarkerTool);
    let tool_ctx = crate::application::run::workspace_test_support::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        Default::default(),
    );
    let wired = factory.build(tool_ctx);
    let _catalog_port = wired.catalog_port();
    let _execution = wired.execution();

    let user_input_id = sdk::InputId::new_v7();
    let user_text = "second-scenario-marker-1272";

    input_tx
        .send(sdk::ChatInputEvent::UserMessage {
            id: user_input_id.clone(),
            text: user_text.to_string(),
            images: Vec::new(),
        })
        .unwrap();

    let shell = test_shell_with_hooks(test_hook_port());
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-per-turn-drain-seal-2");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);

    let driver_sink = sink.clone();
    let driver_after_first = after_first.clone();
    let driver_after_second = after_second.clone();
    let driver = tokio::spawn(async move {
        driver_after_first.notified().await;
        driver_after_second.notified().await;
        loop {
            let done = driver_sink.events().iter().any(|e| e == "DoneWithDuration");
            if done {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver completes");
    driver.await.unwrap();

    // Only one adopted emit even though there are two LLM invocations.
    let adopted = sink.adopted_ids();
    assert_eq!(
        adopted.len(),
        1,
        "post-ToolResults continuation must NOT re-emit UserMessagesAdopted; got {adopted:?}"
    );
    assert_eq!(adopted[0].len(), 1);
    assert_eq!(
        adopted[0][0],
        user_input_id.as_str(),
        "InputId preserved end-to-end"
    );

    let recorded = recorded.lock().unwrap().clone();
    for (idx, payload) in recorded.iter().enumerate() {
        let count = payload
            .iter()
            .filter(|m| matches!(m.role, Role::User) && m.text_content() == user_text)
            .count();
        assert!(
            count <= 1,
            "LLM invocation #{idx} saw user text {count} times — replay regression"
        );
    }
}

/// #1272: idle initial user input → gate adopts → provider returns plain
/// text (no tool calls) → Run terminates after a single LLM invocation.
///
/// Asserts:
///   a) User message appears exactly once in the single LLM payload.
///   b) Exactly 1 LLM invocation.
///   c) `UserMessagesAdopted` emitted exactly once with correct InputId.
///   d) `DoneWithDuration` emitted.
#[tokio::test]
async fn per_turn_drain_seal_context_accept_exactly_once_single_llm_invocation() {
    let after_response = Arc::new(tokio::sync::Notify::new());

    let provider = Arc::new(TextOnlyProvider::new(after_response.clone()));
    let recorded = provider.recorded_messages.clone();
    let call_count = provider.call_count.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    // Empty catalog — no tools needed for text-only path.
    let factory = ::tools::composition::TestCatalogExecutionFactory::empty();
    let _catalog_port = factory.catalog_port();
    let _execution = factory.execution();

    let user_input_id = sdk::InputId::new_v7();
    let user_text = "single-invocation-marker-1272";

    input_tx
        .send(sdk::ChatInputEvent::UserMessage {
            id: user_input_id.clone(),
            text: user_text.to_string(),
            images: Vec::new(),
        })
        .expect("input channel is open");

    let shell = test_shell_with_hooks(test_hook_port());
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-per-turn-drain-seal-single");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);

    let driver_sink = sink.clone();
    let driver_after = after_response.clone();
    let driver = tokio::spawn(async move {
        driver_after.notified().await;
        loop {
            let done = driver_sink.events().iter().any(|e| e == "DoneWithDuration");
            if done {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver completes after single text response");

    driver.await.expect("driver joins cleanly");

    // ── Invariant a: user message appears exactly once ─────────────────
    let recorded = recorded.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1, "expected exactly one LLM invocation");
    let payload = &recorded[0];
    let user_count = payload
        .iter()
        .filter(|m| matches!(m.role, Role::User) && m.text_content() == user_text)
        .count();
    assert_eq!(
        user_count, 1,
        "user message must appear exactly once; found {user_count}"
    );

    // ── Invariant b: exactly 1 LLM invocation ─────────────────────────
    assert_eq!(*call_count.lock().unwrap(), 1);

    // ── Invariant c: UserMessagesAdopted emitted exactly once ──────────
    let adopted = sink.adopted_ids();
    assert_eq!(
        adopted.len(),
        1,
        "UserMessagesAdopted emitted once, got {adopted:?}"
    );
    assert_eq!(
        adopted[0],
        vec![user_input_id.as_str().to_string()],
        "InputId must match the original"
    );

    // ── Invariant d: DoneWithDuration emitted ──────────────────────────
    assert!(
        sink.events().iter().any(|e| e == "DoneWithDuration"),
        "Run must terminate with DoneWithDuration"
    );
}

/// 手动 Compact 必须从 idle gate 进入真实 session loop，并返回可见结果。
/// 仅测试 `apply_gate` 无法覆盖 `PendingCommand` 的执行分支；此场景会
/// 发现命令被接收但没有执行或没有结果事件的断链。
#[tokio::test]
async fn idle_compact_command_reaches_context_and_emits_result() {
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    input_tx.send(sdk::ChatInputEvent::Compact).unwrap();

    let shell = test_shell();
    shell.set_test_session_id("test-idle-compact-command");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);

    let run = tokio::spawn(run_session_command_driver(ctx));
    wait_for_retry_test_condition("manual compact result", || {
        sink.events()
            .iter()
            .any(|event| event == "SystemMessage:Not enough messages to compact.")
    })
    .await;

    drop(input_tx);
    run.await.unwrap();
    assert!(
        sink.events()
            .iter()
            .any(|event| event.starts_with("ActivityChanged:Started:")),
        "manual compact must publish a Runtime-owned Activity"
    );
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| event.starts_with("ActivityChanged:Finished:"))
            .count(),
        1,
        "manual compact Activity must publish exactly one terminal event"
    );
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| event.as_str() == "SystemMessage:Not enough messages to compact.")
            .count(),
        1,
        "manual compact must emit exactly one visible result"
    );
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| event.as_str() == "CommandResultText"),
        "跳过 compact 不得伪报成功"
    );
}

// ─── #1492 task reminder injection ─────────────────────────────────────

/// #1492：run 首步注入 Task 进度 reminder（invocation-only，只给 LLM）：
///  - 首请求 messages 含 `<system-reminder>`（计数 + 任务列表）
///  - 同 run 第二次请求（tool 往返后）不再注入
///  - TUI 同步快照（TurnStarted / SessionMessageStateChanged）不含注入内容
#[tokio::test]
async fn task_reminder_injected_once_per_run_and_never_synced_to_tui() {
    let after_first = Arc::new(tokio::sync::Notify::new());
    let after_second = Arc::new(tokio::sync::Notify::new());

    let provider = Arc::new(ToolThenTextProvider::new(
        after_first.clone(),
        after_second.clone(),
    ));
    let recorded = provider.recorded_messages.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    let factory = ::tools::composition::TestCatalogExecutionFactory::new();
    factory.register(NoopMarkerTool);
    let tool_ctx = crate::application::run::workspace_test_support::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        Default::default(),
    );
    let wired = factory.build(tool_ctx);
    let _catalog_port = wired.catalog_port();
    let _execution = wired.execution();

    // 预置一个 active batch + 一个 in_progress 任务。
    use task::TaskAccess as _;
    let task_store = Arc::new(task::TaskStore::new());
    task_store
        .create_batch(
            task::BatchCreateSpec::try_new("batch".to_string()).unwrap(),
            1,
        )
        .unwrap();
    let task = task_store
        .create_task(
            task::TaskCreateSpec::try_new(
                "修复 compact 收敛".to_string(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap()
        .value;
    task_store
        .transition(task.id(), task::TaskStatus::InProgress, 3)
        .unwrap();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "run with tasks",
            Vec::new(),
        ))
        .unwrap();

    let shell = test_shell_with_task_store(test_hook_port(), task_store.clone());
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-task-reminder-injection");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);

    let driver_sink = sink.clone();
    let driver_after_first = after_first.clone();
    let driver_after_second = after_second.clone();
    let driver = tokio::spawn(async move {
        driver_after_first.notified().await;
        driver_after_second.notified().await;
        loop {
            if driver_sink.events().iter().any(|e| e == "DoneWithDuration") {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        run_session_command_driver(ctx),
    )
    .await
    .expect("run_session_command_driver completes after tool round + final text");
    driver.await.expect("driver joins cleanly");

    let recorded = recorded.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "expected exactly two model invocations (tool call + final text)"
    );

    let reminder_text = "<system-reminder>Current task progress:";
    // 首请求注入；同 run 第二次请求不重复。
    assert!(
        recorded[0]
            .iter()
            .any(|m| m.text_content().contains(reminder_text)),
        "first invocation must carry the task reminder, got: {:?}",
        recorded[0]
            .iter()
            .map(Message::text_content)
            .collect::<Vec<_>>()
    );
    let first_reminder = recorded[0]
        .iter()
        .find(|m| m.text_content().contains(reminder_text))
        .unwrap();
    // 任务处于 in_progress，completed 计数为 0。
    assert!(first_reminder.text_content().contains("━━ Tasks: 0/1 ━━"));
    assert!(first_reminder
        .text_content()
        .contains("■ #1 修复 compact 收敛"));
    assert!(
        !recorded[1]
            .iter()
            .any(|m| m.text_content().contains(reminder_text)),
        "second invocation (tool round continuation) must NOT re-inject the reminder"
    );

    // TUI 同步快照不含注入内容。
    let synced = sink.synced_messages();
    assert!(
        synced
            .iter()
            .flatten()
            .all(|m| !m.text_content().contains(reminder_text)),
        "TUI JSON (synced messages) must not contain the injected reminder"
    );
}

/// #1492：/clear（ChatInputEvent::Reset）在 idle 时清理权威 Task 状态。
#[tokio::test]
async fn clear_resets_authoritative_task_state() {
    let provider = Arc::new(TextOnlyProvider::new(Arc::new(tokio::sync::Notify::new())));

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    use task::TaskAccess as _;
    let task_store = Arc::new(task::TaskStore::new());
    task_store
        .create_batch(
            task::BatchCreateSpec::try_new("batch".to_string()).unwrap(),
            1,
        )
        .unwrap();
    task_store
        .create_task(
            task::TaskCreateSpec::try_new(
                "遗留任务".to_string(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();

    input_tx
        .send(sdk::ChatInputEvent::user_message("first", Vec::new()))
        .unwrap();
    input_tx.send(sdk::ChatInputEvent::Reset).unwrap();

    let shell = test_shell_with_task_store(test_hook_port(), task_store.clone());
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider.clone()),
    );
    shell.set_test_session_id("test-clear-resets-tasks");
    let ctx = test_session_driver_input(sink.clone(), input_events, shell);

    let run = tokio::spawn(run_session_command_driver(ctx));
    wait_for_retry_test_condition("SessionReset observed", || {
        sink.events().iter().any(|event| event == "SessionReset")
    })
    .await;
    drop(input_tx);
    run.await.unwrap();

    assert!(
        task_store.list().is_empty(),
        "/clear must clear the authoritative task aggregate"
    );
}

// ─── #1494 streaming tool execution ───────────────────────────────────

/// 流中发 ToolCallCompleted delta 的 provider：第一次调用在工具 delta 后
/// 延迟（800ms）才发 Completed——旁路执行应发生在这段延迟内（先于流结束）；
/// 第二次调用返回文本（工具结果 continuation 后）。
struct StreamingToolDeltaProvider {
    call_count: Arc<Mutex<usize>>,
    recorded_messages: Arc<Mutex<Vec<Vec<Message>>>>,
    after_tool_completed: Arc<tokio::sync::Notify>,
}

impl StreamingToolDeltaProvider {
    fn new(after_tool_completed: Arc<tokio::sync::Notify>) -> Self {
        Self {
            call_count: Arc::new(Mutex::new(0)),
            recorded_messages: Arc::new(Mutex::new(Vec::new())),
            after_tool_completed,
        }
    }
}

#[async_trait]
impl LlmProvider for StreamingToolDeltaProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let call_num = {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            *count
        };
        self.recorded_messages
            .lock()
            .unwrap()
            .push(messages.to_vec());
        if call_num == 1 {
            self.after_tool_completed.notify_one();
            let tool_call = ProviderToolCall {
                id: ProviderToolCallId("toolu_stream_001".to_string()),
                name: "NoopMarker".to_string(),
                arguments: serde_json::json!({"marker": "noop-marker-result"}),
            };
            let deltas = vec![
                InvocationEvent::Delta(InvocationDelta::ToolCallStarted {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_stream_001".to_string())),
                    name: "NoopMarker".to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolArgumentsDelta {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_stream_001".to_string())),
                    partial_json: r#"{"marker":"noop-marker-result"}"#.to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolCallCompleted {
                    index: 0,
                    call: tool_call.clone(),
                }),
            ];
            let stream = futures::stream::iter(deltas).chain(futures::stream::once(async move {
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::ToolCall(tool_call)],
                    stop_reason: ProviderStopReason::ToolUse,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(10),
                        output_tokens: Some(5),
                        ..RawUsageSnapshot::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                })
            }));
            Ok(Box::pin(stream))
        } else {
            Ok(Box::pin(futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::Text("done after tool".to_string())),
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::Text("done after tool".to_string())],
                    stop_reason: ProviderStopReason::EndTurn,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(20),
                        output_tokens: Some(3),
                        ..RawUsageSnapshot::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ])))
        }
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}
