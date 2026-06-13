# TUI Runtime Observation Context Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the #49 TUI runtime observation context redesign incrementally so runtime streaming events are always applied to their explicit chat/turn context and never rebound through UI active state.

**Architecture:** Start with the correctness slice from spec §9-bis / §11 A-B: remove the `BindRuntimeTurn -> Observe*` two-step protocol, keep explicit `chat_id + turn_id` on every observation intent, and stop runtime observation from changing UI active chat. Then extend lifecycle/progress context through runtime -> SDK -> TUI. Larger structural phases (`RuntimeObservation` projector, `OutputTimelineModel`, `ToolFlowProjector`, render dependency cleanup, `ConversationChange` convergence) are split into follow-up tasks that each keep behavior visually unchanged.

**Tech Stack:** Rust workspace, `cli` crate TUI (`apps/cli/src/tui/**`), `sdk` crate event DTOs (`packages/sdk/src/chat.rs`), runtime event mapping (`agent/features/runtime/src/core/client/event.rs`), cargo test/fmt/clippy.

---

## Current fact-check summary

### 2026-06-13 update after first implementation commit

Completed in `30a36ab8 fix(tui): remove runtime observation active rebinding`:

- `ConversationIntent::ObserveAssistantText`, `ObserveThinkingText`, `CompleteBlock`, `ObserveToolCallStart`, `ObserveToolCallUpdate`, `ObserveToolResult`, and `RecordAgentProgress` now carry explicit `chat_id` + `turn_id`.
- `ConversationIntent::BindRuntimeTurn` has been removed from the live code path.
- `ConversationModel::ensure_runtime_turn()` no longer writes `active_chat_id`.
- Runtime `AgentProgress` now carries context through runtime → SDK → TUI → `ConversationIntent` and writes activity records by explicit context.

Rev.3 spec updates to reflect before continuing:

- `tool_display` ownership stays in render. The `view_assembler -> render` dependency is removed by deleting the assembler-side `lookup_display` call for orphan / non-embedded result summaries, not by moving `tool_display` upward.
- Timeline single-owner rule is hybrid: tool entries store references to `chats`-owned tool data; non-tool text / thinking / system / error / askuser entries may self-own payload because no second owner exists.

Remaining gaps confirmed in current code:

- Runtime lifecycle events `Done`, `DoneWithDuration`, and `Cancelled` still lack context in runtime / SDK / TUI and `ConversationIntent::CompleteChat` still has no explicit context.
- No `RuntimeObservation` application-layer carrier / projector exists yet; `adapter/agent_event.rs` maps `UiEvent` directly to `ConversationIntent`.
- `adapter/agent_event.rs` still imports `crate::tui::render::display::safe_text::safe_str_slice_by_char`.
- `view_assembler/output.rs` still imports render nesting rules and `lookup_display`.
- `ConversationModel.blocks` is still the output timeline and still has tool payload duplicates in `ConversationBlock::ToolCall { name, summary, args_preview }`.
- Tool flow logic is partly extracted to `model/conversation/tool_flow.rs`, but it still mutates `ConversationModel` directly rather than producing atomic conversation + timeline patches.

## Files and responsibilities

- Modify `apps/cli/src/tui/model/conversation/intent.rs`: remove `BindRuntimeTurn`; add explicit context to `RecordAgentProgress`; replace `CompleteChat` with context-aware `CompleteChat { chat_id, turn_id }` only after SDK/runtime lifecycle context exists.
- Modify `apps/cli/src/tui/model/conversation/model.rs`: make `ensure_runtime_turn()` non-activating; route progress/completion by explicit runtime context; keep `active_chat_id` only for UI focus and user input.
- Modify `apps/cli/src/tui/adapter/agent_event.rs`: stop emitting `BindRuntimeTurn`; preserve context directly on each observe intent; later map context-aware progress/completion.
- Modify `apps/cli/src/tui/app/event.rs`: add context to `UiEvent::AgentProgress`; later add context-aware lifecycle variants or a `Lifecycle { context, kind }` enum.
- Modify `apps/cli/src/tui/effect/session/processing.rs`: preserve context from `sdk::ChatEvent::AgentProgress`; later preserve lifecycle context when SDK exposes it.
- Modify `packages/sdk/src/chat.rs`: add context to `ChatEvent::AgentProgress`; later add lifecycle variants with context.
- Modify `agent/features/runtime/src/business/chat/looping/events.rs`: add context to `RuntimeStreamEvent::AgentProgress`; later add context to lifecycle variants.
- Modify `agent/features/runtime/src/business/chat/looping/agent_calls.rs`: include current runtime turn context in forwarded sub-agent progress.
- Modify `agent/features/runtime/src/core/client/event.rs`: map runtime context to SDK context for `AgentProgress`; later for lifecycle.
- Test files: `apps/cli/src/tui/model/conversation/model_tests.rs`, `apps/cli/src/tui/adapter/agent_event.rs` inline tests, `apps/cli/src/tui/effect/session/processing.rs` inline tests if existing or new `#[cfg(test)]` module, `packages/sdk/src/chat.rs` tests.

## Phase A: correctness stopgap, no SDK lifecycle expansion

### Task 1: Add RED tests for non-activating `ensure_runtime_turn`

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/model_tests.rs`

- [ ] **Step 1: Add failing test**

Append this test near the existing active drift tests:

```rust
#[test]
fn test_ensure_runtime_turn_does_not_change_active_chat() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "user focused chat".to_string(),
    });
    let active_before = model.active_chat_id.clone();

    model.ensure_runtime_turn(
        super::ids::ChatId::new("runtime-chat"),
        super::ids::ChatTurnId::new("runtime-turn"),
    );

    assert_eq!(model.active_chat_id, active_before);
    assert!(model
        .chats
        .iter()
        .any(|chat| chat.id == super::ids::ChatId::new("runtime-chat")));
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p cli test_ensure_runtime_turn_does_not_change_active_chat -- --nocapture
```

Expected: FAIL because `ensure_runtime_turn()` currently sets `active_chat_id` to `runtime-chat`.

### Task 2: Make runtime turn creation non-activating

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/model.rs:189-208`

- [ ] **Step 1: Apply minimal implementation**

Change `ensure_runtime_turn()` by deleting the active write. The resulting function body must be:

```rust
    pub(crate) fn ensure_runtime_turn(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
    ) -> (ChatId, ChatTurnId) {
        if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
            chat.status = ChatStatus::Running;
            if !chat.turns.iter().any(|turn| turn.id == turn_id) {
                let sequence = chat.turns.len();
                chat.turns.push(ChatTurn::new(turn_id.clone(), sequence));
            }
            return (chat_id, turn_id);
        }
        let mut chat = Chat::new(chat_id.clone(), String::new());
        chat.turns.clear();
        chat.turns.push(ChatTurn::new(turn_id.clone(), 0));
        self.chats.push(chat);
        (chat_id, turn_id)
    }
```

- [ ] **Step 2: Verify GREEN**

Run:

```bash
cargo test -p cli test_ensure_runtime_turn_does_not_change_active_chat -- --nocapture
```

Expected: PASS.

### Task 3: Add RED adapter tests that forbid `BindRuntimeTurn`

**Files:**
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`

- [ ] **Step 1: Add failing tests in the existing `#[cfg(test)]` module**

Add one helper and one test:

```rust
    fn assert_no_bind_runtime_turn(mapping: &AgentEventMapping) {
        assert!(
            mapping
                .conversation
                .iter()
                .all(|intent| !matches!(intent, ConversationIntent::BindRuntimeTurn { .. })),
            "runtime observations must carry context inline and never emit BindRuntimeTurn"
        );
    }

    #[test]
    fn test_map_agent_event_runtime_observations_do_not_emit_bind_runtime_turn() {
        let context = test_context();

        let events = vec![
            UiEvent::Text {
                context: context.clone(),
                text: "hello".to_string(),
            },
            UiEvent::Thinking {
                context: context.clone(),
                text: "thinking".to_string(),
            },
            UiEvent::BlockComplete {
                context: context.clone(),
                text: String::new(),
            },
            UiEvent::ToolCallStart {
                context: context.clone(),
                id: "tool-1".to_string(),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
            },
            UiEvent::ToolCallUpdate {
                context: context.clone(),
                id: "tool-1".to_string(),
                provider_id: Some("provider-1".to_string()),
                name: "Read".to_string(),
                index: 0,
                arguments_delta: Some(r#"{"file_path":"Cargo.toml"}"#.to_string()),
                arguments: None,
                summary: None,
                status: sdk::ToolCallStatusView::Ready,
            },
            UiEvent::ToolResult {
                context,
                id: "tool-1".to_string(),
                provider_id: "provider-1".to_string(),
                tool_name: "Read".to_string(),
                output: "ok".to_string(),
                content: serde_json::json!({ "text": "ok" }),
                is_error: false,
                images: Vec::new(),
            },
        ];

        for event in events {
            let mapping = map_agent_event(&event);
            assert_no_bind_runtime_turn(&mapping);
        }
    }
```

If the existing module already has `test_context()`, reuse it. If not, add:

```rust
    fn test_context() -> crate::tui::app::event::UiTurnContext {
        crate::tui::app::event::UiTurnContext {
            chat_id: crate::tui::model::conversation::ids::ChatId::new("chat-test"),
            turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-test"),
        }
    }
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p cli test_map_agent_event_runtime_observations_do_not_emit_bind_runtime_turn -- --nocapture
```

Expected: FAIL because `map_agent_event()` currently emits `BindRuntimeTurn`.

### Task 4: Remove `BindRuntimeTurn` from the live adapter and model intent

**Files:**
- Modify: `apps/cli/src/tui/adapter/agent_event.rs:38-212`
- Modify: `apps/cli/src/tui/model/conversation/intent.rs:111-115`
- Modify: `apps/cli/src/tui/model/conversation/model.rs:146-149`
- Modify tests that explicitly construct `BindRuntimeTurn`: `apps/cli/src/tui/model/conversation/model_extra_tests.rs`, possibly old adapter tests.

- [ ] **Step 1: Remove adapter bind prelude**

For each runtime observation branch (`Text`, `Thinking`, `BlockComplete`, `ToolCallStart`, `ToolCallUpdate`, `ToolResult`), replace this pattern:

```rust
let mut mapping = conversation(ConversationIntent::BindRuntimeTurn {
    chat_id: context.chat_id.clone(),
    turn_id: context.turn_id.clone(),
});
mapping.conversation.push(ConversationIntent::ObserveAssistantText { ... });
```

with:

```rust
let mut mapping = conversation(ConversationIntent::ObserveAssistantText {
    chat_id: context.chat_id.clone(),
    turn_id: context.turn_id.clone(),
    text: text.clone(),
});
```

For branches that also push runtime spinner intents, keep pushing those intents after creating the observe mapping.

- [ ] **Step 2: Delete the intent variant**

Remove this variant from `ConversationIntent`:

```rust
    /// Bind subsequent streaming observation events to the runtime chat/turn context.
    BindRuntimeTurn {
        chat_id: super::ids::ChatId,
        turn_id: super::ids::ChatTurnId,
    },
```

- [ ] **Step 3: Delete model match arm**

Remove this arm from `ConversationModel::apply()`:

```rust
              ConversationIntent::BindRuntimeTurn { chat_id, turn_id } => {
                  self.ensure_runtime_turn(chat_id, turn_id);
                  Vec::new()
              }
```

- [ ] **Step 4: Update tests that used bind for setup**

Replace test setup calls like:

```rust
model.apply(ConversationIntent::BindRuntimeTurn {
    chat_id: chat_id.clone(),
    turn_id: turn_id.clone(),
});
```

with:

```rust
model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
```

- [ ] **Step 5: Verify GREEN**

Run:

```bash
cargo test -p cli test_map_agent_event_runtime_observations_do_not_emit_bind_runtime_turn -- --nocapture
cargo test -p cli test_ensure_runtime_turn_does_not_change_active_chat -- --nocapture
```

Expected: both PASS.

### Task 5: Add RED test for agent progress explicit context

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/model_tests.rs`

- [ ] **Step 1: Add failing model test**

Append:

```rust
#[test]
fn test_record_agent_progress_uses_explicit_runtime_context_when_active_turn_drifted() {
    let mut model = ConversationModel::default();
    let live_chat = super::ids::ChatId::new("session-live");
    let live_turn = super::ids::ChatTurnId::new("turn-live");
    let stale_chat = super::ids::ChatId::new("session-stale");
    let stale_turn = super::ids::ChatTurnId::new("turn-stale");

    model.ensure_runtime_turn(live_chat.clone(), live_turn.clone());
    model.apply(ConversationIntent::ObserveToolCallStart {
        chat_id: live_chat.clone(),
        turn_id: live_turn.clone(),
        id: "agent-tool".to_string(),
        provider_id: Some("provider-agent".to_string()),
        name: "Agent".to_string(),
        index: 0,
    });
    model.ensure_runtime_turn(stale_chat.clone(), stale_turn.clone());

    model.apply(ConversationIntent::RecordAgentProgress {
        chat_id: live_chat.clone(),
        turn_id: live_turn.clone(),
        tool_id: "agent-tool".to_string(),
        message: "reading files".to_string(),
    });

    let live_call = model
        .chats
        .iter()
        .find(|chat| chat.id == live_chat)
        .and_then(|chat| chat.turns.iter().find(|turn| turn.id == live_turn))
        .and_then(|turn| turn.tool_calls.iter().find(|call| {
            call.id
                .as_ref()
                .is_some_and(|id| id.as_ref() == "agent-tool")
        }))
        .expect("live agent tool call should exist");

    assert_eq!(live_call.activities, vec!["reading files".to_string()]);
}
```

- [ ] **Step 2: Verify RED compile failure**

Run:

```bash
cargo test -p cli test_record_agent_progress_uses_explicit_runtime_context_when_active_turn_drifted -- --nocapture
```

Expected: FAIL to compile because `RecordAgentProgress` does not yet accept `chat_id` / `turn_id`.

### Task 6: Thread context through AgentProgress runtime -> SDK -> TUI -> model

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/events.rs:124-127`
- Modify: `agent/features/runtime/src/business/chat/looping/agent_calls.rs:110-117`
- Modify: `agent/features/runtime/src/core/client/event.rs:290-295`
- Modify: `packages/sdk/src/chat.rs:329-333`
- Modify: `apps/cli/src/tui/app/event.rs:120-124`
- Modify: `apps/cli/src/tui/effect/session/processing.rs:171-173`
- Modify: `apps/cli/src/tui/adapter/agent_event.rs:257-262`
- Modify: `apps/cli/src/tui/model/conversation/intent.rs:73-76`
- Modify: `apps/cli/src/tui/model/conversation/model.rs:117-119, 477-498`

- [ ] **Step 1: Add context to runtime event**

Change runtime event variant to:

```rust
    AgentProgress {
        context: RuntimeTurnContext,
        tool_id: String,
        event: AgentProgressEvent,
    },
```

In `agent_calls.rs`, change forwarded event to:

```rust
                .send_event(RuntimeStreamEvent::AgentProgress {
                    context: context.clone(),
                    tool_id: call_id.clone(),
                    event,
                })
```

- [ ] **Step 2: Add context to SDK event**

Change SDK variant to:

```rust
    AgentProgress {
        context: ChatEventContext,
        tool_id: String,
        event: AgentProgressEventView,
    },
```

In runtime SDK mapping, change to:

```rust
        crate::business::chat::RuntimeStreamEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => ChatEvent::AgentProgress {
            context: ChatEventContext::new(context.chat_id, context.turn_id),
            tool_id,
            event: agent_progress_event_to_sdk(event),
        },
```

- [ ] **Step 3: Add context to TUI event and mapping**

Change TUI event to:

```rust
      AgentProgress {
          context: UiTurnContext,
          tool_id: String,
          event: sdk::AgentProgressEventView,
      },
```

Change SDK-to-UI mapping to:

```rust
          sdk::ChatEvent::AgentProgress {
              context,
              tool_id,
              event,
          } => UiEvent::AgentProgress {
              context: context.into(),
              tool_id,
              event,
          },
```

Change adapter mapping to:

```rust
          UiEvent::AgentProgress { context, tool_id, event } => {
              conversation(ConversationIntent::RecordAgentProgress {
                  chat_id: context.chat_id.clone(),
                  turn_id: context.turn_id.clone(),
                  tool_id: tool_id.clone(),
                  message: format!("{event}"),
              })
          }
```

- [ ] **Step 4: Add context to conversation intent and model handler**

Change intent variant to:

```rust
      RecordAgentProgress {
          chat_id: ChatId,
          turn_id: ChatTurnId,
          tool_id: String,
          message: String,
      },
```

Change apply arm to pass context:

```rust
              ConversationIntent::RecordAgentProgress {
                  chat_id,
                  turn_id,
                  tool_id,
                  message,
              } => self.record_agent_progress(chat_id, turn_id, tool_id, message),
```

Change handler to:

```rust
      fn record_agent_progress(
          &mut self,
          chat_id: ChatId,
          turn_id: ChatTurnId,
          tool_id: String,
          message: String,
      ) -> Vec<ConversationChange> {
          if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
              if let Some(call) = turn
                  .tool_calls
                  .iter_mut()
                  .find(|c| c.id.as_ref().is_some_and(|id| id.as_ref() == tool_id))
              {
                  call.activities.push(message.clone());
              }
          }
          self.agent_progress
              .push(AgentProgressEntry::new(tool_id.clone(), message.clone()));
          vec![ConversationChange::OutputDirty]
      }
```

- [ ] **Step 5: Update existing construction sites**

Search with the repository content-search tool for this pattern under `apps`, `packages`, and `agent`:

```text
RecordAgentProgress|AgentProgress \{
```

Every `RecordAgentProgress` construction must include explicit `chat_id` and `turn_id`. Every `UiEvent::AgentProgress`, `sdk::ChatEvent::AgentProgress`, and `RuntimeStreamEvent::AgentProgress` construction must include context.

- [ ] **Step 6: Verify GREEN**

Run:

```bash
cargo test -p cli test_record_agent_progress_uses_explicit_runtime_context_when_active_turn_drifted -- --nocapture
cargo test -p cli agent_progress -- --nocapture
cargo test -p sdk agent_progress -- --nocapture
```

Expected: PASS.

## Phase B: legacy-safe completion and lifecycle context follow-up

### Task 7: Add RED test showing legacy completion must not complete active stale chat

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/model_tests.rs`

- [ ] **Step 1: Add failing test for current active-based completion**

```rust
#[test]
fn test_legacy_complete_chat_does_not_complete_active_stale_chat() {
    let mut model = ConversationModel::default();
    let live_chat = super::ids::ChatId::new("session-live");
    let live_turn = super::ids::ChatTurnId::new("turn-live");
    let stale_chat = super::ids::ChatId::new("session-stale");
    let stale_turn = super::ids::ChatTurnId::new("turn-stale");

    model.ensure_runtime_turn(live_chat.clone(), live_turn);
    model.ensure_runtime_turn(stale_chat.clone(), stale_turn);
    model.active_chat_id = Some(stale_chat.clone());

    model.apply(ConversationIntent::CompleteChat);

    let stale = model
        .chats
        .iter()
        .find(|chat| chat.id == stale_chat)
        .expect("stale chat exists");
    assert_ne!(stale.status, super::chat::ChatStatus::Completing);
}
```

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p cli test_legacy_complete_chat_does_not_complete_active_stale_chat -- --nocapture
```

Expected: FAIL because current `CompleteChat` uses `active_chat_mut()`.

### Task 8: Stop legacy lifecycle from mutating specific turns until context exists

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/model.rs:382-391`
- Modify: `apps/cli/src/tui/adapter/agent_event.rs:273-275` only if needed for naming/comment.

- [ ] **Step 1: Make legacy complete clear streaming block state only**

Replace `complete_chat()` with:

```rust
      fn complete_chat(&mut self) -> Vec<ConversationChange> {
          self.active_text_block_id = None;
          self.active_text_context = None;
          self.active_thinking_block_id = None;
          self.active_thinking_context = None;
          Vec::new()
      }
```

This matches spec §10.3: lifecycle events without context are legacy and must not mutate a concrete turn/chat. Global spinner/status updates remain handled outside the conversation model.

- [ ] **Step 2: Verify GREEN**

Run:

```bash
cargo test -p cli test_legacy_complete_chat_does_not_complete_active_stale_chat -- --nocapture
cargo test -p cli complete_chat -- --nocapture
```

Expected: targeted test PASS. If older tests expected active chat completion, update them to assert legacy lifecycle no longer mutates specific chat status.

### Task 9: Add context-preservation tests for SDK-to-UI AgentProgress

**Files:**
- Modify: `apps/cli/src/tui/effect/session/processing.rs`

- [ ] **Step 1: Add test module if absent**

Append:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sdk_event_to_ui_event_preserves_agent_progress_context() {
        let ui_event = sdk_event_to_ui_event(sdk::ChatEvent::AgentProgress {
            context: sdk::ChatEventContext::new("chat-progress", "turn-progress"),
            tool_id: "tool-1".to_string(),
            event: sdk::AgentProgressEventView {
                timestamp: 1,
                kind: sdk::AgentProgressKindView::Message {
                    text: "working".to_string(),
                },
            },
        });

        match ui_event {
            UiEvent::AgentProgress { context, tool_id, .. } => {
                assert_eq!(context.chat_id.as_ref(), "chat-progress");
                assert_eq!(context.turn_id.as_ref(), "turn-progress");
                assert_eq!(tool_id, "tool-1");
            }
            other => panic!("expected AgentProgress, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Verify test**

Run:

```bash
cargo test -p cli test_sdk_event_to_ui_event_preserves_agent_progress_context -- --nocapture
```

Expected: PASS after Task 6.

### Task 10: Lifecycle context expansion design checkpoint

**Files:**
- Modify later: `agent/features/runtime/src/business/chat/looping/events.rs`, `agent/features/runtime/src/business/chat/looping/finalize.rs`, `agent/features/runtime/src/business/chat/looping/loop_runner.rs`, `agent/features/runtime/src/core/client/event.rs`, `packages/sdk/src/chat.rs`, `apps/cli/src/tui/app/event.rs`, `apps/cli/src/tui/effect/session/processing.rs`, `apps/cli/src/tui/adapter/agent_event.rs`, `apps/cli/src/tui/model/conversation/intent.rs`, `apps/cli/src/tui/model/conversation/model.rs`.

- [ ] **Step 1: Do not implement lifecycle context in the same commit as Phase A unless runtime current-turn context is clearly available at every emit site**

Reason: `RuntimeStreamEvent::Cancelled` is emitted from multiple loop-runner branches. A partial context patch risks inventing the wrong context. Keep the safe legacy behavior from Task 8 until every lifecycle emit site can use the authoritative `RuntimeTurnContext` already held by the loop.

- [ ] **Step 2: When implementing, replace lifecycle variants instead of overloading legacy variants**

Target API:

```rust
Done { context: RuntimeTurnContext }
DoneWithDuration { context: RuntimeTurnContext, duration: std::time::Duration }
Cancelled { context: RuntimeTurnContext }
```

SDK target:

```rust
Done { context: ChatEventContext }
DoneWithDurationMs { context: ChatEventContext, duration_ms: u64 }
Cancelled { context: ChatEventContext }
```

TUI target:

```rust
Done { context: UiTurnContext }
DoneWithDuration { context: UiTurnContext, duration: std::time::Duration }
Cancelled { context: UiTurnContext }
```

Conversation target:

```rust
CompleteChat { chat_id: ChatId, turn_id: ChatTurnId }
```

Model completion target must use `runtime_turn_mut(&chat_id, &turn_id)` or find the owning chat by `chat_id`; it must not use `active_chat_mut()`.

## Phase C: RuntimeObservation application-layer carrier

### Task 11: Introduce `RuntimeTurnContext` type alias/struct in TUI model boundary

**Files:**
- Create: `apps/cli/src/tui/model/runtime_observation.rs` or `apps/cli/src/tui/app/runtime_observation.rs`
- Modify: `apps/cli/src/tui/model.rs` or `apps/cli/src/tui/app.rs` module exports.

- [ ] **Step 1: Add tests first**

Create tests that construct each runtime observation variant and assert `context.chat_id` / `context.turn_id` are accessible.

- [ ] **Step 2: Add `RuntimeTurnContext` and `RuntimeObservation`**

Use a single strongly typed context:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeTurnContext {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}
```

Then define variants from spec §7.2. Do not add projector logic in this task.

### Task 12: Map `UiEvent` to `RuntimeObservation` before conversation commands

**Files:**
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Test: existing inline tests.

- [ ] **Step 1: RED context preservation tests**

For every runtime streaming event, assert mapping produces one observation with identical context.

- [ ] **Step 2: GREEN mapping**

Refactor adapter internals to build `RuntimeObservation` first, then convert to existing `ConversationIntent` so behavior remains unchanged.

## Phase C2: Context-aware lifecycle events

### Task 12.1: Thread lifecycle context from runtime to SDK

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/events.rs`
- Modify: `agent/features/runtime/src/business/chat/looping/finalize.rs`
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`
- Modify: `agent/features/runtime/src/core/client/event.rs`
- Modify: `packages/sdk/src/chat.rs`

- [ ] **Step 1: RED compile/API test**

Add or update tests so `RuntimeStreamEvent::Done`, `DoneWithDuration`, and `Cancelled` require `RuntimeTurnContext`, and SDK `ChatEvent::Done`, `DoneWithDurationMs`, and `Cancelled` require `ChatEventContext`.

- [ ] **Step 2: GREEN runtime emit sites**

Use the authoritative `RuntimeTurnContext` already passed through chat loop functions:

```rust
Done { context: context.clone() }
DoneWithDuration { context: context.clone(), duration }
Cancelled { context: context.clone() }
```

Do not synthesize context from UI active state or session-global mutable state.

### Task 12.2: Thread lifecycle context through TUI and completion model

**Files:**
- Modify: `apps/cli/src/tui/app/event.rs`
- Modify: `apps/cli/src/tui/effect/session/processing.rs`
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Modify: `apps/cli/src/tui/model/conversation/intent.rs`
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
- Modify tests in `apps/cli/src/tui/model/conversation/model_tests.rs` and adapter/processing inline tests.

- [ ] **Step 1: RED stale-active completion test**

Assert a lifecycle complete event for `live_chat/live_turn` marks only that chat as completed/completing while `active_chat_id` points at `stale_chat`.

- [ ] **Step 2: GREEN context-aware completion**

Change `ConversationIntent::CompleteChat` to carry `chat_id` and `turn_id`, then implement completion by locating `chat_id` directly. It must never call `active_chat_mut()` or `active_turn_mut()`.

## Phase D: dependency direction cleanup

### Task 13: Move safe text utility out of render

**Files:**
- Create: `apps/cli/src/tui/text/safe_text.rs`
- Modify: `apps/cli/src/tui/text.rs` or module export file.
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Keep old render path only as a temporary re-export if many render callers exist.

- [ ] **Step 1: RED import guard**

Add or update a test/guard that fails if `apps/cli/src/tui/adapter/**` imports `crate::tui::render::`.

- [ ] **Step 2: Move implementation**

Move `safe_str_slice_by_char` to `tui::text::safe_text`; update adapter import to `crate::tui::text::safe_text::safe_str_slice_by_char`.

### Task 14: Remove view assembler's render dependency while keeping `tool_display` in render

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output.rs`
- Create/modify: `apps/cli/src/tui/view_assembler/output_nesting.rs` or `apps/cli/src/tui/view_model/output_nesting.rs` if nesting rules are still needed outside render.
- Do **not** move `apps/cli/src/tui/render/tool_display.rs`; rev.3 explicitly keeps display logic render-owned.

- [ ] **Step 1: RED import guard**

Assert `apps/cli/src/tui/view_assembler/**` does not contain `crate::tui::render::` imports.

- [ ] **Step 2: Remove assembler-side display lookup**

For orphan / non-embedded tool results, stop calling render `lookup_display` from the assembler. Preserve raw/sanitized result text in the model/view model and let render-side `tool_display` decide final presentation when drawing.

- [ ] **Step 3: Move only generic nesting constants if still needed**

If assembler still needs `allowed_child` / `MAX_BLOCK_DEPTH`, move those generic rules to a non-render module. Render may import them downward; assembler must not import render.

## Phase E: OutputTimelineModel and ToolFlowProjector follow-up

### Task 15: Split timeline ownership without changing visual output

**Files:**
- Create: `apps/cli/src/tui/model/output_timeline/mod.rs`
- Create focused submodules as needed: `item.rs`, `model.rs`, `intent.rs`.
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
- Modify: `apps/cli/src/tui/view_assembler/output.rs`

- [ ] **Step 1: RED single-owner tests for tool entries**

Assert tool call timeline entries store stable references (`chat_id`, `turn_id`, `tool_call_id` / index) and do not duplicate mutable tool payload fields (`name`, `summary`, `args_preview`, `result`).

- [ ] **Step 2: GREEN minimal split**

Move ordering, active text/thinking block ids, and orphan result placement into timeline model. Keep `ConversationModel.chats` as the single owner for tool call / result data. Non-tool text, thinking, system, error, and ask-user entries may keep their payload in the timeline because they have no second owner.

### Task 16: Extract ToolFlowProjector with atomic patches

**Files:**
- Create: `apps/cli/src/tui/update/runtime_observation.rs` or `apps/cli/src/tui/app/runtime_observation.rs`
- Modify: `apps/cli/src/tui/model/conversation/tool_flow.rs`
- Modify: `apps/cli/src/tui/model/output_timeline/**`

- [ ] **Step 1: RED collision and atomicity tests**

Add tests for repeated tool IDs across turns/chats and for no timeline reference without a corresponding `ChatTurn.tool_calls` entry.

- [ ] **Step 2: GREEN projector**

Move tool start/update/result binding logic out of `ConversationModel` into a projector that creates both conversation and timeline patches and applies them in one reducer call.

## Verification gate for each phase

Run after each phase:

```bash
cargo fmt
cargo test -p cli
cargo test -p sdk
cargo test -p runtime
cargo clippy -p cli --all-targets -- -D warnings
```

If a phase touches runtime or SDK APIs, also run:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

## Commit strategy

- Commit Phase A as one correctness commit after all Phase A tests pass.
- Commit AgentProgress context as a separate API-threading commit if it touches runtime/SDK/CLI broadly.
- Do not mix Phase D/E structural refactors into the correctness commit.
- Use repository commit style, e.g. `fix(tui): remove runtime observation active rebinding`.

## Self-review checklist

- Every production behavior change has a RED test first.
- `BindRuntimeTurn` has zero occurrences outside the plan/spec history after Phase A.
- `ensure_runtime_turn()` does not write `active_chat_id`.
- Runtime observation paths do not call `active_chat_mut()` / `active_turn_mut()` for ownership.
- Legacy lifecycle without context does not mutate a specific chat/turn.
- Tool result ordering and existing TUI snapshots remain visually unchanged.
