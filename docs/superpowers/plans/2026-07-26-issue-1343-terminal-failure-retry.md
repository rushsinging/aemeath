# Model Terminal Failure Retry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make retryable stream interruptions retry even after visible output, and make empty terminal model completions retry through the same Runtime invocation coordinator.

**Architecture:** Provider remains responsible for structurally classifying transport EOF as retryable `StreamTruncated`. Runtime's shared `InvocationEventReducer` rejects completions that contain neither non-blank assistant text nor a tool call, and `RetryPolicy` ignores visible-delta commitment when deciding retry eligibility. Main and Sub continue using their existing coordinator, backoff, cancellation, and retry notification paths; TUI retains already-rendered fragments and appends the retry notice and replacement attempt output.

**Tech Stack:** Rust 2021, Tokio, futures streams, reqwest test fixtures, Runtime/Provider Published Language, ratatui TUI scenario harness, Cargo workspace tests, shell architecture guards.

---

## File Structure

- Modify `agent/features/provider/src/adapters/stream_contract_tests.rs`: preserve the real chunked-body EOF contract and add a fatal malformed-protocol counterexample.
- Modify `agent/features/runtime/src/application/model_invocation.rs`: change retry eligibility while retaining visible-delta state as diagnostic output; update the existing policy/coordinator tests in the same legacy test module.
- Modify `agent/features/runtime/src/application/main_loop/looping/stream_handler.rs`: introduce the single semantic terminal-completion validator and make the reducer return a retryable invocation failure before projecting an invalid completion.
- Modify `agent/features/runtime/src/application/main_loop/looping/stream_handler_tests.rs`: test empty, whitespace-only, thinking-only, text, and tool-only completion contracts.
- Modify `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`: prove the Main adapter retries after committed output and after an empty completion, preserves partial output, emits retry status, and eventually succeeds/fails correctly.
- Modify `agent/features/runtime/src/application/subagent/runner/tests.rs`: add a scripted provider/runner fixture and prove Sub uses the same empty-terminal retry and exhaustion behavior.
- Modify `agent/features/runtime/src/adapters/event_projection_tests.rs`: prove the Runtime retry event keeps attempt, delay, and turn identity at the Runtime → SDK boundary.
- Modify `apps/cli/src/tui/adapter/event_mapping_tests.rs`: prove the SDK → TUI ACL preserves the same retry fields.
- Modify `apps/cli/src/tui/app/scenario_tests/chat.rs`: prove the retry notice is appended between retained partial and successful retry output with no rollback/replacement.
- Create `apps/cli/src/tui/app/scenario_tests/snapshots/aemeath__tui__app__scenario_tests__chat__chat_retry_after_partial__100x30.snap`: lock the accepted duplicate/append-only presentation.

No public SDK type, Session schema, provider error enum, or TUI model field is added.

### Task 1: Lock the Provider transport/protocol boundary

**Files:**
- Modify: `agent/features/provider/src/adapters/stream_contract_tests.rs:1-180`

- [ ] **Step 1: Strengthen the real EOF contract test**

Rename the existing test to state the exact invariant and assert both the structured classification and the diagnostic cause:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openai_chunked_body_eof_is_retryable_stream_truncated() {
    // Keep the existing TcpListener fixture that advertises an incomplete
    // chunk, then collect the OpenAiChat invocation stream.
    let [InvocationEvent::Failed(error)] = events.as_slice() else {
        panic!("chunked EOF must emit exactly one failed terminal: {events:?}");
    };
    assert_eq!(error.kind, ProviderErrorKind::StreamTruncated);
    assert!(error.retryable);
    assert!(
        error.safe_message.contains("unexpected EOF")
            || error.safe_message.contains("connection"),
        "transport cause must remain diagnosable: {error:?}"
    );
}
```

- [ ] **Step 2: Add the malformed payload counterexample**

Use `response_from_fixture` with a complete HTTP body containing an invalid OpenAI SSE JSON record and assert it terminates as fatal `Protocol`, not `StreamTruncated`:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn openai_complete_body_with_malformed_json_is_fatal_protocol_error() {
    let response = response_from_fixture("data: {not-json}\n\n", "text/event-stream").await;
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
            if error.kind == ProviderErrorKind::Protocol && !error.retryable
    ));
}
```

- [ ] **Step 3: Run the Provider contract tests**

Run:

```bash
cargo test -p provider adapters::stream::contract_tests -- --nocapture
```

Expected: both tests pass on the current structural mapping. If the malformed fixture is intentionally ignored by the decoder, replace it with the smallest complete malformed event already rejected by `parse_openai_stream`; do not change production classification merely to satisfy the fixture.

- [ ] **Step 4: Commit the contract evidence**

```bash
git add agent/features/provider/src/adapters/stream_contract_tests.rs
git commit -m "test(provider): #1343 lock stream EOF classification"
```

### Task 2: Retry structurally retryable failures after visible deltas

**Files:**
- Modify: `agent/features/runtime/src/application/model_invocation.rs:157-185`
- Modify: `agent/features/runtime/src/application/model_invocation.rs:510-655`

- [ ] **Step 1: Change the existing tests to express the desired policy**

Replace `visible_delta_and_fatal_error_disable_retry` with separate positive and negative cases, and rename the committed-delta coordinator test:

```rust
#[test]
fn visible_delta_does_not_disable_structurally_retryable_error() {
    let policy = RetryPolicy::default();
    assert_eq!(
        policy.decide(
            1,
            true,
            &retryable(ProviderErrorKind::StreamTruncated),
            0,
        ),
        RetryDecision::RetryAfter(Duration::from_secs(10))
    );
}

#[test]
fn fatal_error_still_fails_after_visible_delta() {
    let policy = RetryPolicy::default();
    assert_eq!(
        policy.decide(
            1,
            true,
            &ProviderError::fatal(ProviderErrorKind::Authentication, "safe"),
            0,
        ),
        RetryDecision::Fail
    );
}

#[tokio::test]
async fn main_committed_delta_remains_diagnostic_but_can_retry() {
    // Keep the current partial-delta + missing-terminal fixture.
    assert!(committed_delta);
    assert_eq!(
        coordinator.policy.decide(1, committed_delta, &error, 0),
        RetryDecision::RetryAfter(Duration::from_secs(10))
    );
}
```

- [ ] **Step 2: Run the focused tests to capture RED**

Run:

```bash
cargo test -p runtime visible_delta_does_not_disable_structurally_retryable_error
cargo test -p runtime main_committed_delta_remains_diagnostic_but_can_retry
```

Expected: both fail because `RetryPolicy::decide` still returns `Fail` when `visible_delta` is true.

- [ ] **Step 3: Remove only the visible-delta veto**

Keep the parameter as diagnostic data for callers, but do not use it in eligibility:

```rust
pub(crate) fn decide(
    &self,
    attempt: u32,
    _visible_delta: bool,
    error: &ProviderError,
    jitter_millis: u64,
) -> RetryDecision {
    if error.kind == ProviderErrorKind::ContextTooLong {
        return RetryDecision::Compact;
    }
    if error.kind == ProviderErrorKind::RateLimited
        || !error.retryable
        || attempt >= self.max_attempts
    {
        return RetryDecision::Fail;
    }
    // Preserve the existing exponential/retry-after/jitter calculation.
}
```

Update `pull_stream`'s doc comment to say committed delta is retained for diagnostics and future rollback-aware presentation, not retry eligibility.

- [ ] **Step 4: Run all model invocation tests**

Run:

```bash
cargo test -p runtime application::model_invocation::tests -- --nocapture
```

Expected: all model invocation tests pass, including attempt limit and cancellation during backoff.

- [ ] **Step 5: Commit the retry policy change**

```bash
git add agent/features/runtime/src/application/model_invocation.rs
git commit -m "fix(runtime): #1343 retry after visible model deltas"
```

### Task 3: Reject semantically empty completion at the shared reducer boundary

**Files:**
- Modify: `agent/features/runtime/src/application/main_loop/looping/stream_handler_tests.rs:1-160`
- Modify: `agent/features/runtime/src/application/main_loop/looping/stream_handler.rs:95-180`

- [ ] **Step 1: Add the empty-terminal RED matrix**

Import `ProviderErrorKind` in `stream_handler_tests.rs` and add a table-driven test:

```rust
#[test]
fn reducer_rejects_terminal_completion_without_text_or_tool_call() {
    let cases = [
        Vec::new(),
        vec![ProviderContentBlock::Text(String::new())],
        vec![ProviderContentBlock::Text("   \n".into())],
        vec![ProviderContentBlock::Thinking {
            thinking: "internal only".into(),
            signature: None,
        }],
    ];

    for output in cases {
        let mut reducer = InvocationEventReducer::new(RecordingSink::default());
        let error = reducer
            .apply(completion(output))
            .expect_err("empty terminal completion must be retryable failure");
        assert_eq!(error.kind, ProviderErrorKind::Protocol);
        assert!(error.retryable);
        assert!(error.safe_message.contains("assistant text or tool call"));
    }
}

#[test]
fn reducer_accepts_non_blank_text_and_tool_only_completion() {
    for output in [
        vec![ProviderContentBlock::Text("answer".into())],
        vec![ProviderContentBlock::ToolCall(ProviderToolCall {
            id: ProviderToolCallId("tool-1".into()),
            name: "Read".into(),
            arguments: serde_json::json!({"file_path": "Cargo.toml"}),
        })],
    ] {
        let mut reducer = InvocationEventReducer::new(RecordingSink::default());
        assert!(reducer.apply(completion(output)).unwrap().is_some());
    }
}
```

- [ ] **Step 2: Run the reducer test to capture RED**

Run:

```bash
cargo test -p runtime reducer_rejects_terminal_completion_without_text_or_tool_call -- --nocapture
```

Expected: fail because the reducer currently builds a successful `InvocationResponse` for all four invalid outputs.

- [ ] **Step 3: Add one shared completion predicate and synthetic error constructor**

In `stream_handler.rs`, define these private functions once:

```rust
fn has_actionable_terminal_output(output: &[provider::ProviderContentBlock]) -> bool {
    output.iter().any(|block| match block {
        provider::ProviderContentBlock::Text(text) => !text.trim().is_empty(),
        provider::ProviderContentBlock::ToolCall(_) => true,
        provider::ProviderContentBlock::Thinking { .. } => false,
    })
}

fn empty_terminal_error() -> provider::ProviderError {
    provider::ProviderError::retryable(
        provider::ProviderErrorKind::Protocol,
        "provider completed without assistant text or tool call",
    )
}
```

At the beginning of `InvocationEvent::Completed(completion)`, close any active streaming block, then reject before fallback projection or response assembly:

```rust
InvocationEvent::Completed(completion) => {
    self.handler.complete_active_streaming_block();
    if !has_actionable_terminal_output(&completion.output) {
        return Err(empty_terminal_error());
    }
    // Existing fallback projection and InvocationResponse assembly remain unchanged.
}
```

This ordering preserves already-projected thinking/text and guarantees the streaming block is closed before backoff.

- [ ] **Step 4: Update the existing thinking progress test**

`reducer_progress_tracks_visible_deltas_and_waiting_phase` currently completes with thinking-only output. Change its terminal fixture to include valid text:

```rust
reducer
    .apply(InvocationEvent::Delta(InvocationDelta::Text("answer".into())))
    .unwrap();
reducer
    .apply(completion(vec![ProviderContentBlock::Text("answer".into())]))
    .unwrap();
```

Keep the assertion that the completed block returns to `waiting_model_output`.

- [ ] **Step 5: Run reducer and Runtime focused tests**

Run:

```bash
cargo test -p runtime stream_handler -- --nocapture
cargo test -p runtime application::model_invocation::tests -- --nocapture
```

Expected: all pass; valid text and tool-only completions remain successful.

- [ ] **Step 6: Commit the empty terminal classification**

```bash
git add agent/features/runtime/src/application/main_loop/looping/stream_handler.rs agent/features/runtime/src/application/main_loop/looping/stream_handler_tests.rs
git commit -m "fix(runtime): #1343 classify empty model completion as retryable"
```

### Task 4: Prove Main retries partial streams and empty completions

**Files:**
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs:519-655`
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs:2959-3070`
- Modify if Tokio test utilities are not already enabled: `Cargo.toml:28`
- Modify if Tokio test utilities are not already enabled: `Cargo.lock`

- [ ] **Step 1: Make retry events observable in the existing Main RecordingSink**

Replace the wildcard handling of retry events with:

```rust
RuntimeStreamEvent::ModelInvocationRetrying { attempt, delay, .. } => {
    format!("ModelInvocationRetrying:{attempt}:{}", delay.as_millis())
}
```

- [ ] **Step 2: Add a scripted Main provider for partial-stream and empty-completion attempts**

Define a provider whose call selects a full `InvocationStream`:

```rust
#[derive(Clone)]
struct RetryThenSuccessProvider {
    attempts: Arc<Mutex<VecDeque<Vec<InvocationEvent>>>>,
    calls: Arc<Mutex<usize>>,
}

#[async_trait]
impl LlmProvider for RetryThenSuccessProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        *self.calls.lock().unwrap() += 1;
        let events = self.attempts.lock().unwrap().pop_front().expect("scripted attempt");
        Ok(Box::pin(futures::stream::iter(events)))
    }

    fn model_name(&self) -> &str { "test-model" }
    fn provider_name(&self) -> &str { "test-provider" }
}

fn retryable_stream_failure() -> InvocationEvent {
    InvocationEvent::Failed(ProviderError::retryable(
        ProviderErrorKind::StreamTruncated,
        "stream connection interrupted: unexpected EOF during chunk size line",
    ))
}

fn empty_completion() -> InvocationEvent {
    InvocationEvent::Completed(ProviderCompletion {
        output: Vec::new(),
        stop_reason: ProviderStopReason::EndTurn,
        usage: Some(RawUsageSnapshot::default()),
        effective_reasoning: ReasoningLevel::Off,
    })
}
```

- [ ] **Step 3: Add the Main partial-output retry RED scenario**

Use `#[tokio::test(start_paused = true)]`, submit one user input, and script attempt 1 as `Text("partial")` then `retryable_stream_failure()`, and attempt 2 as `Text("complete")` plus a matching successful completion. Drive virtual time until the first provider call, call `tokio::time::advance(Duration::from_secs(10)).await`, then wait for completion and close input. Assert:

```rust
assert_eq!(*provider.calls.lock().unwrap(), 2);
let events = sink.events();
let partial = events.iter().position(|e| e == "Text:partial").unwrap();
let retry = events.iter().position(|e| e.starts_with("ModelInvocationRetrying:2:")).unwrap();
let complete = events.iter().position(|e| e == "Text:complete").unwrap();
assert!(partial < retry && retry < complete);
assert!(!events.iter().any(|e| e.starts_with("ApiError:")));
```

- [ ] **Step 4: Run the partial-output scenario to capture RED**

Run:

```bash
cargo test -p runtime main_partial_stream_failure_retries_without_rollback -- --nocapture
```

Expected: fail before a retry event because committed visible output still disables retry until Task 2 is applied.

If `tokio::time::advance` is unavailable, add Tokio's `test-util` feature at the workspace dependency declaration in `Cargo.toml`, update `Cargo.lock`, and include both files in this task's commit. Do not replace virtual time with wall-clock sleeps.

- [ ] **Step 5: Add Main empty-success and exhaustion scenarios**

First script `[empty completion, successful text completion]`; assert attempt 2, retry event, final text, and no empty assistant message committed. Then script eleven empty completions under paused time, advance through each documented backoff (10, 20, 40, 80, then 120 seconds for remaining waits), and assert:

```rust
assert_eq!(*provider.calls.lock().unwrap(), 11);
assert_eq!(
    sink.events()
        .iter()
        .filter(|event| event.starts_with("ModelInvocationRetrying:"))
        .count(),
    10
);
assert!(sink.events().iter().any(|event| {
    event.starts_with("ApiError:")
        && event.contains("provider completed without assistant text or tool call")
}));
```

- [ ] **Step 6: Run the Main scenarios**

Run:

```bash
cargo test -p runtime main_partial_stream_failure_retries_without_rollback -- --nocapture
cargo test -p runtime main_empty_completion_retries_and_succeeds -- --nocapture
cargo test -p runtime main_empty_completion_exhaustion_fails_instead_of_completing -- --nocapture
```

Expected: all pass without wall-clock sleeps; exhaustion starts exactly 11 provider attempts.

- [ ] **Step 7: Commit Main integration coverage**

```bash
git add agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs
git commit -m "test(runtime): #1343 cover main terminal retries"
```

### Task 5: Prove Sub retries empty completion and fails on exhaustion

**Files:**
- Modify: `agent/features/runtime/src/application/subagent/runner/tests.rs:1-140`
- Modify: `agent/features/runtime/src/application/subagent/runner/tests.rs:1390-1580`

- [ ] **Step 1: Add a reusable scripted Sub provider and runner constructor**

Add imports for the Provider completion/event types, `VecDeque`, and `Mutex`. Define `ScriptedCompletionProvider` like the Main scripted provider, and add:

```rust
fn test_runner_with_provider(
    provider: Arc<dyn LlmProvider>,
) -> (
    CliAgentRunner,
    crate::application::runtime_context::ParentRunFrameGuard,
) {
    let (src, guard) = test_parent_source();
    (
        CliAgentRunner {
            factory: crate::application::testing::constant_factory(
                crate::application::testing::binding_from_llm_provider(provider),
            ),
            config_reader: test_config_reader(),
            active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
            max_tool_concurrency: 10,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            tool_result_materializer:
                crate::application::testing::test_tool_result_materializer(),
            workspace: crate::application::testing::runtime_workspace(
                &crate::application::testing::test_tool_execution_context(
                    std::env::temp_dir(),
                    CancellationToken::new(),
                ),
            ),
            skill_materializer: empty_skill_materializer(),
            parent_context: src,
        },
        guard,
    )
}
```

Refactor `test_runner(error)` to call this helper with `Arc::new(ErrorProvider { error })`; do not duplicate runner assembly.

- [ ] **Step 2: Add the Sub empty-success RED scenario**

Use `#[tokio::test(start_paused = true)]`, script one empty completion followed by a valid text completion, spawn `run_agent`, yield until one provider call is observed, advance 10 seconds, and assert:

```rust
assert_eq!(result, tools::AgentRunTerminal::Completed("sub recovered".into()));
assert_eq!(*calls.lock().unwrap(), 2);
```

Run:

```bash
cargo test -p runtime sub_empty_completion_retries_and_succeeds -- --nocapture
```

Expected RED before Task 3: returns `Completed("")` after one attempt.

- [ ] **Step 3: Add the Sub exhaustion scenario**

Script eleven empty completions, advance the same retry schedule with paused Tokio time, and assert:

```rust
assert_eq!(*calls.lock().unwrap(), 11);
assert_eq!(
    result,
    tools::AgentRunTerminal::Failed {
        error: "loop adapter error: protocol error: provider completed without assistant text or tool call".into(),
    }
);
```

- [ ] **Step 4: Run Sub tests**

Run:

```bash
cargo test -p runtime sub_empty_completion_retries_and_succeeds -- --nocapture
cargo test -p runtime sub_empty_completion_exhaustion_is_typed_failure -- --nocapture
cargo test -p runtime application::subagent::runner::tests -- --nocapture
```

Expected: all pass; existing cancellation and provider fatal-error semantics remain unchanged.

- [ ] **Step 5: Commit Sub integration coverage**

```bash
git add agent/features/runtime/src/application/subagent/runner/tests.rs
git commit -m "test(runtime): #1343 cover sub-agent empty retries"
```

### Task 6: Verify Runtime → SDK → TUI retry projection and append-only rendering

**Files:**
- Modify: `agent/features/runtime/src/adapters/event_projection_tests.rs:1-130`
- Modify: `apps/cli/src/tui/adapter/event_mapping_tests.rs:1-100`
- Modify: `apps/cli/src/tui/app/scenario_tests/chat.rs:1-82`
- Create: `apps/cli/src/tui/app/scenario_tests/snapshots/aemeath__tui__app__scenario_tests__chat__chat_retry_after_partial__100x30.snap`

- [ ] **Step 1: Add the Runtime → SDK adjacent contract test**

```rust
#[test]
fn model_retry_projection_preserves_context_attempt_and_delay() {
    let context = RuntimeTurnContext::new(
        sdk::ids::ChatId::new("chat-retry"),
        sdk::ids::ChatTurnId::new("turn-retry"),
    );
    let event = RuntimeStreamEvent::ModelInvocationRetrying {
        context,
        attempt: 2,
        delay: std::time::Duration::from_millis(10_250),
    };

    assert!(matches!(
        project_stream_event(event),
        sdk::ChatEvent::ModelInvocationRetrying { context, attempt: 2, delay }
            if context.chat_id.as_str() == "chat-retry"
                && context.turn_id.as_str() == "turn-retry"
                && delay == std::time::Duration::from_millis(10_250)
    ));
}
```

- [ ] **Step 2: Add the SDK → TUI adjacent contract test**

```rust
#[test]
fn model_retry_mapping_preserves_context_attempt_and_delay() {
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::ModelInvocationRetrying {
        context: sdk::ChatEventContext::new(
            sdk::ids::ChatId::new("chat-retry"),
            sdk::ids::ChatTurnId::new("turn-retry"),
        ),
        attempt: 2,
        delay: std::time::Duration::from_millis(10_250),
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::ModelInvocationRetrying {
            context,
            attempt: 2,
            delay_ms: 10_250,
        }) if context.chat_id == "chat-retry" && context.turn_id == "turn-retry"
    ));
}
```

- [ ] **Step 3: Add the L4 TUI append-only scenario**

```rust
#[test]
fn retry_after_partial_output_keeps_partial_notice_and_new_attempt() {
    let mut harness = TuiScenarioHarness::new(100, 30);
    harness.runtime_event(TuiRuntimeEvent::TurnStarted { messages: vec![] });
    harness.runtime_event(TuiRuntimeEvent::Text {
        context: ctx(),
        text: "partial answer".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::BlockComplete {
        context: ctx(),
        text: String::new(),
    });
    harness.runtime_event(TuiRuntimeEvent::ModelInvocationRetrying {
        context: ctx(),
        attempt: 2,
        delay_ms: 10_000,
    });
    harness.runtime_event(TuiRuntimeEvent::Text {
        context: ctx(),
        text: "replacement complete answer".into(),
    });
    harness.runtime_event(TuiRuntimeEvent::BlockComplete {
        context: ctx(),
        text: String::new(),
    });
    harness.runtime_event(TuiRuntimeEvent::Done {
        context: ctx(),
        duration_ms: None,
    });
    harness.render();

    let screen = harness.screen();
    assert!(screen.contains("partial answer"));
    assert!(screen.contains("Retrying model invocation (attempt 2) in 10.0s."));
    assert!(screen.contains("replacement complete answer"));
    insta::assert_snapshot!("chat_retry_after_partial__100x30", screen);
    harness.assert_idle();
}
```

- [ ] **Step 4: Run adjacent and scenario tests, then accept only the new snapshot**

Run:

```bash
cargo test -p runtime model_retry_projection_preserves_context_attempt_and_delay
cargo test -p cli model_retry_mapping_preserves_context_attempt_and_delay
cargo test -p cli retry_after_partial_output_keeps_partial_notice_and_new_attempt
cargo insta pending-snapshots --manifest-path apps/cli/Cargo.toml
```

Expected: the two adjacent contracts pass. The scenario first creates one `.snap.new`; inspect it, confirm all three lines appear in order with no removal marker, then run:

```bash
cargo insta accept --manifest-path apps/cli/Cargo.toml
cargo test -p cli retry_after_partial_output_keeps_partial_notice_and_new_attempt
```

Expected: the new scenario passes and no unrelated snapshots change.

- [ ] **Step 5: Commit the cross-layer evidence**

```bash
git add agent/features/runtime/src/adapters/event_projection_tests.rs apps/cli/src/tui/adapter/event_mapping_tests.rs apps/cli/src/tui/app/scenario_tests/chat.rs apps/cli/src/tui/app/scenario_tests/snapshots/aemeath__tui__app__scenario_tests__chat__chat_retry_after_partial__100x30.snap
git commit -m "test(tui): #1343 preserve output across model retry"
```

### Task 7: Remove obsolete diagnostics and run all gates

**Files:**
- Modify if now unreachable: `agent/features/runtime/src/application/loop_engine/engine.rs:606-623`
- Modify if now unreachable: `agent/features/runtime/src/application/subagent/runner/loop_run.rs:545-575`
- Modify if comments are stale: `agent/features/runtime/src/application/loop_engine/llm_strategy.rs:1-50`

- [ ] **Step 1: Check obsolete empty-success paths and stale rollback wording**

Run:

```bash
rg -n "empty_terminal_text|subagent_empty_complete_text|rollbackable|disables_retry|cannot be rolled back|committed_delta" agent/features/runtime/src
```

Expected: the two empty-terminal warning blocks are unreachable after reducer validation and should be removed. Update `LlmStrategy::committed_delta` comments so they describe presentation diagnostics, not retry safety. Do not remove the committed-delta value itself unless all production and test consumers prove it has no remaining diagnostic value.

- [ ] **Step 2: Apply formatting and verify no diff defects**

Run:

```bash
cargo fmt --all
cargo fmt --all -- --check
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 3: Run development environment and targeted crate gates**

Run:

```bash
scripts/setup-dev-env.sh --check
cargo test -p provider
cargo test -p runtime
cargo test -p composition
cargo test -p cli
cargo check -p cli
cargo clippy -p provider --all-targets -- -D warnings
cargo clippy -p runtime --all-targets -- -D warnings
cargo clippy -p cli --all-targets -- -D warnings
```

Expected: all exit 0. A first failure must be investigated and fixed; a later pass does not erase the original failure record.

- [ ] **Step 4: Run workspace and architecture gates**

Run:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
.agents/hooks/check-architecture-guards.sh --full
```

Expected: all tests, clippy checks, and registered guards pass.

- [ ] **Step 5: Review scope and commit cleanup**

Run:

```bash
git diff --stat origin/main...HEAD
git diff --check origin/main...HEAD
git status --short
rg -n "placeholder-marker|incomplete-marker|fixme-marker" docs/superpowers/specs/2026-07-26-issue-1343-terminal-failure-retry-design.md docs/superpowers/plans/2026-07-26-issue-1343-terminal-failure-retry.md
```

Expected: only #1343 design, plan, implementation, tests, and the intentional snapshot are present; no generated `.snap.new`, target artifacts, or unrelated files remain.

If Task 7 changed production comments or removed warnings, commit them:

```bash
git add agent/features/runtime/src/application/loop_engine/engine.rs agent/features/runtime/src/application/subagent/runner/loop_run.rs agent/features/runtime/src/application/loop_engine/llm_strategy.rs
git commit -m "refactor(runtime): #1343 retire empty completion diagnostics"
```

### Task 8: Synchronize with main and open the PR

**Files:**
- Modify only if conflict resolution is required: files already listed above
- Use: `.github/pull_request_template.md`

- [ ] **Step 1: Pull latest main into the bugfix branch**

Run:

```bash
git pull --no-rebase origin main
```

Expected: fast-forward or merge succeeds. If conflicts occur, resolve only in #1343-owned files, rerun Task 7's full affected gates, and commit the merge resolution.

- [ ] **Step 2: Recheck the Issue gate before PR creation**

Run:

```bash
gh issue view 1343 --repo rushsinging/aemeath --json number,title,state,milestone,body,url
```

Expected: Issue is open, still attached to `v0.1.0 — Context Engineering + 架构重构`, and has no unchecked body checklist. Do not close it.

- [ ] **Step 3: Push and create a non-draft PR to main**

Create `/tmp/aemeath-1343-pr.md` from `.github/pull_request_template.md` with this content:

```markdown
## Summary

- retry structurally retryable stream failures after visible deltas
- classify terminal completions without assistant text or tool calls as retryable invocation failures
- preserve append-only TUI output while exposing the existing retry notice

## Refs

Refs #1343

## Breaking change

No.

## Test plan

- `cargo test -p provider`
- `cargo test -p runtime`
- `cargo test -p composition`
- `cargo test -p cli`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `.agents/hooks/check-architecture-guards.sh --full`
```

Then run:

```bash
git push -u origin fix/1343-retry-terminal-failures
gh pr create --repo rushsinging/aemeath --base main --head fix/1343-retry-terminal-failures --title "fix(runtime): retry interrupted and empty model responses" --body-file /tmp/aemeath-1343-pr.md
```

- [ ] **Step 4: Verify PR metadata and checks**

Run:

```bash
gh pr view --repo rushsinging/aemeath --json number,url,state,isDraft,baseRefName,headRefName,headRefOid,mergeable,mergeStateStatus,statusCheckRollup
```

Expected: PR targets `main`, head is `fix/1343-retry-terminal-failures`, it is not Draft, and required checks are visible. Report the PR URL and any pending checks; do not merge and do not close Issue #1343 without explicit user authorization.
