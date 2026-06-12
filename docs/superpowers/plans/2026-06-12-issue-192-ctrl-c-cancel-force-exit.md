# Issue 192 Ctrl+C Cancel and Force Exit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Ctrl+C a reliable control flow: first press cancels the active LLM/tool/sub-agent run, second press while cancelling exits the whole TUI without waiting for runtime cleanup.

**Architecture:** Introduce explicit TUI ownership of the processing task, a small Ctrl+C runtime-state reducer, and explicit cancelled semantics across provider/runtime/tool boundaries. Runtime cancellation becomes a normal outcome (`Cancelled`) rather than an error path, and tool execution wrappers observe the shared `CancellationToken` before timeout.

**Tech Stack:** Rust 2021, Tokio, tokio-util `CancellationToken`, crossterm key events, ratatui TUI, workspace packages `cli`, `runtime`, `provider`, `tools`.

---

## File Structure

- Modify `apps/cli/src/tui/app/update/key.rs`
  - Own the Ctrl+C decision reducer for idle, running, and cancelling states.
  - Add tests for first running Ctrl+C and second cancelling Ctrl+C.
- Modify `apps/cli/src/tui/effect/session/processing.rs`
  - Make spawned chat forwarding return an owned handle.
  - Track whether the background forwarding task has been locally aborted.
- Modify `apps/cli/src/tui/app/state/chat.rs`
  - Add optional processing handle and cancelling flag to `ChatState`.
- Modify `apps/cli/src/tui/effect/executor.rs`
  - Implement cancel request and force-exit cleanup through the processing handle.
- Modify `apps/cli/src/tui/app/update/ui_event.rs`
  - Clear processing handle and cancelling state on `Done`, `DoneWithDuration`, `Cancelled`, and `Error`.
- Modify `agent/features/provider/src/lib.rs`
  - Add explicit provider cancellation error classification.
- Modify provider stream implementations:
  - `agent/features/provider/src/business/providers/openai_compatible/stream.rs`
  - `agent/features/provider/src/business/providers/anthropic.rs`
  - `agent/features/provider/src/business/providers/ollama/stream.rs`
  - Return the explicit cancellation error when the shared token fires.
- Modify `agent/features/runtime/src/business/chat/looping/loop_runner.rs`
  - Map provider cancellation to `RuntimeStreamEvent::Cancelled` and `AgentRunStatus::Cancelled`.
  - Do not emit `Error` or run stop-failure hooks for user cancellation.
- Modify `agent/features/runtime/src/business/agent/agent.rs`
  - Make `call_tool_with_timeout` cancel-aware by selecting on `ctx.cancel.cancelled()` before timeout.
  - Return a structured internal cancelled result for tool calls.
- Modify `agent/features/runtime/src/business/chat/looping/non_agent.rs`
  - Stop launching new tool calls once cancellation is requested.
- Modify `agent/features/runtime/src/business/agent/runner/loop_run.rs`
  - Map cancelled provider errors from sub-agent LLM streams to `AgentRunStatus::Cancelled`.

## Task 1: TUI Ctrl+C reducer and processing lifecycle state

**Files:**
- Modify: `apps/cli/src/tui/app/update/key.rs`
- Modify: `apps/cli/src/tui/app/state/chat.rs`
- Test: `apps/cli/src/tui/app/update/key.rs`

- [ ] **Step 1: Confirm baseline key tests**

Run:

```bash
cargo test -p cli --lib tui::app::update::key::tests::test_ctrlc_action_empty_first_press_warns -- --exact
```

Expected: existing test passes.

- [ ] **Step 2: Write failing reducer tests**

In `apps/cli/src/tui/app/update/key.rs`, extend `CtrlCAction` to represent processing states. Replace the existing enum with:

```rust
#[derive(Debug, Clone, PartialEq)]
pub(super) enum CtrlCAction {
    ClearInput,
    WarnExit,
    Quit,
    RequestCancel,
    ForceQuit,
}
```

Replace `ctrlc_action` with this signature and logic skeleton so the new tests compile only after implementation:

```rust
fn ctrlc_action(
    input_empty: bool,
    last_ctrlc: Option<std::time::Instant>,
    is_processing: bool,
    is_cancelling: bool,
) -> CtrlCAction {
    let _ = (input_empty, last_ctrlc, is_processing, is_cancelling);
    CtrlCAction::WarnExit
}
```

Update old tests to pass `false, false` for the new parameters, then add:

```rust
#[test]
fn test_ctrlc_action_processing_first_press_requests_cancel() {
    assert_eq!(ctrlc_action(true, None, true, false), CtrlCAction::RequestCancel);
    assert_eq!(ctrlc_action(false, None, true, false), CtrlCAction::RequestCancel);
}

#[test]
fn test_ctrlc_action_cancelling_second_press_force_quits() {
    assert_eq!(ctrlc_action(true, None, true, true), CtrlCAction::ForceQuit);
    assert_eq!(ctrlc_action(false, None, true, true), CtrlCAction::ForceQuit);
}
```

- [ ] **Step 3: Run failing tests**

Run:

```bash
cargo test -p cli test_ctrlc_action_processing_first_press_requests_cancel test_ctrlc_action_cancelling_second_press_force_quits
```

Expected: both new tests fail because `ctrlc_action` always returns `WarnExit`.

- [ ] **Step 4: Implement reducer**

Replace the skeleton body with:

```rust
fn ctrlc_action(
    input_empty: bool,
    last_ctrlc: Option<std::time::Instant>,
    is_processing: bool,
    is_cancelling: bool,
) -> CtrlCAction {
    if is_processing {
        return if is_cancelling {
            CtrlCAction::ForceQuit
        } else {
            CtrlCAction::RequestCancel
        };
    }

    if !input_empty {
        return CtrlCAction::ClearInput;
    }

    let now = std::time::Instant::now();
    if let Some(last) = last_ctrlc {
        if now.duration_since(last).as_secs_f64() < CTRL_C_TIMEOUT_SECS {
            return CtrlCAction::Quit;
        }
    }
    CtrlCAction::WarnExit
}
```

Update all old call sites from:

```rust
ctrlc_action(self.model.input.document.is_empty(), self.layout.last_ctrlc)
```

to:

```rust
ctrlc_action(
    self.model.input.document.is_empty(),
    self.layout.last_ctrlc,
    self.chat.is_processing,
    self.chat.is_cancelling,
)
```

Add this field to `apps/cli/src/tui/app/state/chat.rs` in `ChatState`:

```rust
pub is_cancelling: bool,
```

Initialize it as `false`, set it to `false` in `start_processing()` and `stop_processing()`, and add:

```rust
pub(crate) fn start_cancelling(&mut self) {
    self.is_cancelling = true;
}
```

- [ ] **Step 5: Wire Ctrl+C actions in `update_key`**

Replace the processing-specific branch:

```rust
if self.chat.is_processing {
    if let Some(agent_client) = &spawn_refs.agent_client {
        agent_client.cancel();
    }
    self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
        StatusNotice::warning("Interrupted"),
    ));
} else if completion_visible {
```

with a single match over `ctrlc_action(...)`:

```rust
let action = ctrlc_action(
    self.model.input.document.is_empty(),
    self.layout.last_ctrlc,
    self.chat.is_processing,
    self.chat.is_cancelling,
);
match action {
    CtrlCAction::RequestCancel => {
        self.chat.start_cancelling();
        self.layout.mark_ctrlc_now();
        return UpdateResult::one(Effect::CancelAgentChat);
    }
    CtrlCAction::ForceQuit => {
        return UpdateResult::one(Effect::QuitApplication);
    }
    CtrlCAction::ClearInput => {
        self.handle_input_intent(InputIntent::Clear);
        self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
            StatusNotice::warning("Input cleared (Ctrl+C again to exit)"),
        ));
        self.layout.mark_ctrlc_now();
    }
    CtrlCAction::WarnExit => {
        if completion_visible {
            self.handle_input_intent(InputIntent::SetCompletions {
                query: String::new(),
                items: Vec::new(),
            });
        } else {
            self.layout.mark_ctrlc_now();
            self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
                StatusNotice::warning("Press Ctrl+C again to exit"),
            ));
        }
    }
    CtrlCAction::Quit => {
        return UpdateResult::one(Effect::QuitApplication);
    }
}
```

Then adjust the message in `Effect::CancelAgentChat` execution in Task 2 to show cancellation text, not here.

- [ ] **Step 6: Run reducer tests**

Run:

```bash
cargo test -p cli ctrlc_action
```

Expected: all Ctrl+C reducer tests pass.

## Task 2: TUI processing task ownership and force-exit cleanup

**Files:**
- Modify: `apps/cli/src/tui/effect/session/processing.rs`
- Modify: `apps/cli/src/tui/effect/executor.rs`
- Modify: chat state file from Task 1
- Modify: `apps/cli/src/tui/app/run_loop.rs`
- Modify: `apps/cli/src/tui/app/update/ui_event.rs`
- Test: `apps/cli/src/tui/effect/session/processing.rs` and existing executor tests

- [ ] **Step 1: Add processing handle type**

In `apps/cli/src/tui/effect/session/processing.rs`, add before `pub fn spawn_processing`:

```rust
pub(crate) struct ProcessingHandle {
    join: tokio::task::JoinHandle<()>,
}

impl ProcessingHandle {
    pub(crate) fn abort(&self) {
        self.join.abort();
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.join.is_finished()
    }
}
```

Change:

```rust
pub fn spawn_processing(ctx: SpawnContext) {
    tokio::spawn(async move {
```

to:

```rust
pub fn spawn_processing(ctx: SpawnContext) -> ProcessingHandle {
    let join = tokio::spawn(async move {
```

and add after the spawned async block:

```rust
    ProcessingHandle { join }
}
```

- [ ] **Step 2: Store handle in chat state**

In the chat state struct that owns `is_processing`, add:

```rust
pub processing_handle: Option<crate::tui::effect::session::processing::ProcessingHandle>,
```

Initialize as `None`. Add methods:

```rust
pub(crate) fn set_processing_handle(
    &mut self,
    handle: crate::tui::effect::session::processing::ProcessingHandle,
) {
    self.processing_handle = Some(handle);
}

pub(crate) fn abort_processing_handle(&mut self) {
    if let Some(handle) = self.processing_handle.take() {
        handle.abort();
    }
}

pub(crate) fn clear_processing_handle(&mut self) {
    self.processing_handle = None;
    self.is_cancelling = false;
}
```

- [ ] **Step 3: Update spawn call sites**

In `apps/cli/src/tui/app/run_loop.rs`, replace:

```rust
processing::spawn_processing(spawn_ctx);
```

with:

```rust
let handle = processing::spawn_processing(spawn_ctx);
self.chat.set_processing_handle(handle);
```

In `apps/cli/src/tui/effect/executor.rs`, replace:

```rust
processing::spawn_processing(spawn_ctx);
```

with:

```rust
let handle = processing::spawn_processing(spawn_ctx);
self.chat.set_processing_handle(handle);
```

- [ ] **Step 4: Make cancel and force quit own the handle**

In `apps/cli/src/tui/effect/executor.rs`, change `Effect::QuitApplication` handling from:

```rust
Effect::QuitApplication => self.layout.request_exit(),
```

to:

```rust
Effect::QuitApplication => {
    self.chat.abort_processing_handle();
    self.layout.request_exit();
}
```

Change `cancel_agent_chat` to:

```rust
fn cancel_agent_chat(&mut self) {
    self.chat.start_cancelling();
    if let Some(ref ac) = self.agent_client {
        ac.cancel();
    }
    self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
        StatusNotice::warning("Cancelling current response… Press Ctrl+C again to exit"),
    ));
}
```

- [ ] **Step 5: Clear handle on terminal events**

In `apps/cli/src/tui/app/update/ui_event.rs`, add `self.chat.clear_processing_handle();` in these branches after stopping processing:

```rust
UiEvent::Error(msg) => {
    ...
    self.chat.stop_processing();
    self.chat.clear_processing_handle();
    ...
}

UiEvent::Cancelled => {
    ...
    self.chat.stop_processing();
    self.chat.clear_processing_handle();
}

UiEvent::ReflectionDone { output } => {
    ...
    self.chat.stop_processing();
    self.chat.clear_processing_handle();
    ...
}
```

Also find the `UiEvent::Done` and `UiEvent::DoneWithDuration` branches in the same file and add the same cleanup after `self.chat.stop_processing()`.

- [ ] **Step 6: Run CLI tests**

Run:

```bash
cargo test -p cli
```

Expected: all CLI tests pass.

## Task 3: Explicit provider cancellation error type

**Files:**
- Modify: `agent/features/provider/src/lib.rs`
- Modify: `agent/features/provider/src/business/providers/openai_compatible/stream.rs`
- Modify: `agent/features/provider/src/business/providers/anthropic.rs`
- Modify: `agent/features/provider/src/business/providers/ollama/stream.rs`
- Test: provider crate unit tests in the same files or nearby test modules

- [ ] **Step 1: Add explicit error variant**

In `agent/features/provider/src/lib.rs`, update `LlmError`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("network error: {0}")]
    Network(String),
    #[error("API error [{error_type}]: {message}")]
    Api { error_type: String, message: String },
    #[error("rate limited")]
    RateLimited,
    #[error("context too long")]
    ContextTooLong,
    #[error("stream error: {0}")]
    Stream(String),
    #[error("request cancelled by user")]
    Cancelled,
    #[error("config error: {0}")]
    Config(String),
}

impl LlmError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, LlmError::Cancelled)
    }
}
```

- [ ] **Step 2: Return cancellation variant from stream implementations**

In each provider stream implementation, find the `tokio::select!` branch that waits on `cancel.cancelled()` or checks `cancel.is_cancelled()` and currently returns an ordinary error string like `"interrupted by user"`.

Replace that return with:

```rust
return Err(crate::LlmError::Cancelled);
```

Use `crate::LlmError::Cancelled` in provider crate files, or `provider::api::LlmError::Cancelled` outside provider if that is the exported path.

- [ ] **Step 3: Add classification test**

In `agent/features/provider/src/lib.rs` tests, add:

```rust
#[test]
fn llm_cancelled_error_is_classified_as_cancelled() {
    let error = LlmError::Cancelled;
    assert!(error.is_cancelled());
}
```

The new test uses the concrete `LlmError` type added in Step 1.

- [ ] **Step 4: Run provider tests**

Run:

```bash
cargo test -p provider
```

Expected: provider tests pass.

## Task 4: Runtime maps cancellation to Cancelled event, not Error

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`
- Test: same file test module

- [ ] **Step 1: Add failing runtime test**

In `agent/features/runtime/src/business/chat/looping/loop_runner.rs` test module, add a focused test for the helper introduced in Step 2:

```rust
#[test]
fn provider_cancelled_error_maps_to_cancelled_outcome() {
    let error = provider::api::LlmError::Cancelled;
    assert!(is_user_cancelled_provider_error(&error));
}
```

The test uses the exported `provider::api::LlmError` path.

- [ ] **Step 2: Add helper**

Near `chat_loop_transition_for_gate_exit`, add:

```rust
fn is_user_cancelled_provider_error(error: &provider::api::LlmError) -> bool {
    error.is_cancelled()
}
```

`stream_message` returns `provider::api::LlmError`; use that concrete type in helpers and tests.

- [ ] **Step 3: Rewrite `Err(e)` branch**

In the `match response { Err(e) => { ... } }` branch, insert before `let error_msg = e.to_string();`:

```rust
if is_user_cancelled_provider_error(&e) || cancel.is_cancelled() || interrupted.load(Ordering::Relaxed) {
    interrupted.store(false, Ordering::Relaxed);
    messages.truncate(messages_at_start);
    sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone())).await;
    sink.send_event(RuntimeStreamEvent::Cancelled).await;
    loop_fsm.transition(ChatLoopTransition::CancelCurrentLoop);
    let outcome = AgentRunOutcome {
        status: AgentRunStatus::Cancelled,
        turns: turn_count,
        duration: turn_start.elapsed(),
        role: None,
        model: client.model_name().to_string(),
    };
    log_agent_outcome(&outcome, &session_id);
    let _ = sink.send_event(RuntimeStreamEvent::Done).await;
    loop_fsm.transition(ChatLoopTransition::StopSucceeded);
    loop_fsm.assert_state(ChatLoopState::Done, "cancelled provider error finalizes loop");
    break;
}
```

This path MUST NOT call `finalize_main_loop` with `ApiError`, because that would run stop-failure hooks.

- [ ] **Step 4: Run runtime tests**

Run:

```bash
cargo test -p runtime provider_cancelled_error_maps_to_cancelled_outcome
cargo test -p runtime
```

Expected: both commands pass.

## Task 5: Cancel-aware tool execution wrapper

**Files:**
- Modify: `agent/features/runtime/src/business/agent/agent.rs`
- Modify: `agent/features/runtime/src/business/chat/looping/non_agent.rs`
- Test: `agent/features/runtime/src/business/agent/agent_tests.rs` or the existing test module for `agent.rs`

- [ ] **Step 1: Add tool cancellation message helper**

In `agent/features/runtime/src/business/agent/agent.rs`, add near `tool_call_timeout_message`:

```rust
fn tool_call_cancelled_message(name: &str) -> String {
    format!("tool.call execution cancelled: tool={name}")
}
```

- [ ] **Step 2: Make `call_tool_with_timeout` cancellation-aware**

Replace the body of `call_tool_with_timeout` with:

```rust
async fn call_tool_with_timeout(
    tool: std::sync::Arc<dyn Tool>,
    name: &str,
    input: serde_json::Value,
    ctx: &ToolExecutionContext,
) -> Result<ToolResult, String> {
    if ctx.cancel.is_cancelled() {
        return Err(tool_call_cancelled_message(name));
    }

    let timeout = tool.timeout_secs();
    let started = std::time::Instant::now();
    tokio::select! {
        _ = ctx.cancel.cancelled() => {
            let message = tool_call_cancelled_message(name);
            log::info!("{message}");
            Err(message)
        }
        result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            tool.call(input, ctx),
        ) => {
            match result {
                Ok(result) => {
                    log::debug!(
                        "tool.call execution finished: tool={}, timeout_secs={}, elapsed_ms={}",
                        name,
                        timeout,
                        started.elapsed().as_millis()
                    );
                    Ok(result)
                }
                Err(_) => {
                    let elapsed = started.elapsed();
                    let message = tool_call_timeout_message(name, timeout, elapsed);
                    log::warn!("{message}");
                    Err(message)
                }
            }
        }
    }
}
```

- [ ] **Step 3: Stop launching new non-agent tools after cancel**

In `apps/cli` no change. In `agent/features/runtime/src/business/chat/looping/non_agent.rs`, before executing each sequential tool, add:

```rust
if agent.ctx.cancel.is_cancelled() {
    return Vec::new();
}
```

Inside the concurrent future before acquiring the semaphore, add:

```rust
if agent.ctx.cancel.is_cancelled() {
    return (pos, Vec::new());
}
```

Then adjust result collection to tolerate an empty result vector by leaving the slot as a cancelled result if necessary:

```rust
if let Some(r) = result_vec.into_iter().next() {
    results[pos] = Some(r);
} else {
    let call = other_calls[pos];
    results[pos] = Some((
        call.id.clone(),
        call.provider_id.clone(),
        "Cancelled by user".to_string(),
        serde_json::json!({ "text": "Cancelled by user" }),
        true,
        Vec::new(),
    ));
}
```

Apply the same fallback in the sequential result collection block.

- [ ] **Step 4: Add or update tests**

In the existing `agent_tests.rs`, add a test for `tool_call_cancelled_message` if private helper tests are in same module:

```rust
#[test]
fn tool_cancelled_message_names_tool() {
    assert_eq!(
        super::tool_call_cancelled_message("Bash"),
        "tool.call execution cancelled: tool=Bash"
    );
}
```

- [ ] **Step 5: Run runtime tests**

Run:

```bash
cargo test -p runtime tool_cancelled_message_names_tool
cargo test -p runtime
```

Expected: runtime tests pass.

## Task 6: Sub-agent cancellation is normal cancellation

**Files:**
- Modify: `agent/features/runtime/src/business/agent/runner/loop_run.rs`
- Test: same file or existing sub-agent runner tests

- [ ] **Step 1: Update sub-agent provider error mapping**

In `agent/features/runtime/src/business/agent/runner/loop_run.rs`, replace the `Err(e)` branch:

```rust
Err(e) => {
    (self.progress)(Some(turn_number), &format!("Agent error: {e}"));
    let error_string = e.to_string();
    let result = format!("Sub-agent error: {error_string}");
    finalize_and_return!(AgentRunStatus::ApiError(error_string), turn, result);
}
```

with:

```rust
Err(e) => {
    if e.is_cancelled() || self.ctx.cancel.is_cancelled() {
        (self.progress)(Some(turn_number), "Agent cancelled by user");
        let result = "Cancelled by user".to_string();
        finalize_and_return!(AgentRunStatus::Cancelled, turn, result);
    }
    (self.progress)(Some(turn_number), &format!("Agent error: {e}"));
    let error_string = e.to_string();
    let result = format!("Sub-agent error: {error_string}");
    finalize_and_return!(AgentRunStatus::ApiError(error_string), turn, result);
}
```

- [ ] **Step 2: Run runtime tests**

Run:

```bash
cargo test -p runtime
```

Expected: runtime tests pass.

## Task 7: End-to-end verification and formatting

**Files:**
- No new files. All modified Rust files from Tasks 1-6.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt --all
```

Expected: command exits 0.

- [ ] **Step 2: Run package tests**

Run:

```bash
cargo test -p cli
cargo test -p provider
cargo test -p runtime
```

Expected: all tests pass.

- [ ] **Step 3: Run workspace check**

Run:

```bash
cargo check --workspace
```

Expected: command exits 0.

- [ ] **Step 4: Manual TUI smoke test**

Run:

```bash
cargo run -p cli --bin aemeath
```

Manual verification:

1. Start a prompt that streams for several seconds.
2. Press `Ctrl+C` once while text or thinking is streaming.
3. Expected: status says `Cancelling current response… Press Ctrl+C again to exit` and no error hook message appears.
4. Press `Ctrl+C` again before runtime finishes cancellation.
5. Expected: the whole TUI exits immediately.
6. Reopen TUI, run another prompt, press `Ctrl+C` once, then wait.
7. Expected: current response is cancelled, UI returns to ready state, and conversation shows a normal cancellation notice.

- [ ] **Step 5: Inspect git diff**

Run:

```bash
git diff --stat
git diff -- apps/cli/src/tui agent/features/provider agent/features/runtime
```

Expected: diff only touches Ctrl+C lifecycle, cancellation typing, runtime cancellation mapping, and cancel-aware tool/sub-agent paths.
