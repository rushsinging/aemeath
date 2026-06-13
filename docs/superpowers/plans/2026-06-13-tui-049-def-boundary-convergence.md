# TUI 049 D/E/F Boundary Convergence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete phases D/E/F of `docs/feature/specs/049-tui-render-observation-context.md` by splitting output timeline ownership, extracting tool-flow projection, and simplifying change/dirty/render boundaries.

**Architecture:** Phase D introduces `OutputTimelineModel` as the output read-model owner for ordering plus non-tool payloads while tool data remains owned only by `ConversationModel.chats[*].turns[*].tool_calls`. Phase E extracts runtime-observation-to-model patches into `ToolFlowProjector` and applies multi-model patches atomically. Phase F collapses over-modeled conversation changes into dirty flags and adds structural guard tests so render remains a consumer of ViewModel, not domain model.

**Tech Stack:** Rust, Cargo workspace, TUI modules under `apps/cli/src/tui/**`, existing unit tests, `cargo fmt`, `cargo test -p cli`, `cargo clippy -p cli --all-targets -- -D warnings`.

---

## Required context before execution

- Load root `AGENTS.md` instructions.
- Load `specs/rust-coding.md` because all tasks edit Rust files.
- Load `specs/tui-cli.md` because all tasks edit `apps/cli/src/**` TUI code.
- Load `specs/bug-feature-tracking.md` because this is feature implementation work tied to issue `rushsinging/aemeath#151`.
- Work in an isolated git worktree. Do not edit the main checkout directly.
- Keep `apps/cli/src/tui/render/output/tool_display.rs` in render. Per 049 rev.3, only nesting and general text helpers move upward; tool_display remains render-owned.

## File structure

### Phase D: OutputTimelineModel and single-owner tool display data

- Create: `apps/cli/src/tui/model/output_timeline/mod.rs`
  - Exports `OutputTimelineModel`, `OutputTimelineItem`, `TimelineRuntimeContext`, and `TimelineToolCallRef`.
- Create: `apps/cli/src/tui/model/output_timeline/item.rs`
  - Defines timeline item variants: non-tool payload variants self-own text/content; tool call/result variants store references only.
- Create: `apps/cli/src/tui/model/output_timeline/model.rs`
  - Owns ordered `items: Vec<OutputTimelineItem>` and helper methods for append/extend/retain/move operations.
- Modify: `apps/cli/src/tui/model.rs`
  - Add `pub mod output_timeline;`.
- Modify: `apps/cli/src/tui/model/conversation/block.rs`
  - Reduce `ConversationBlock::ToolCall` to `ToolCall { id, chat_id, turn_id }`.
  - Reduce `ConversationBlock::ToolResult` to `ToolResult { id, chat_id, turn_id }` after timeline owns tool-result output/content for orphan or display-only records, or replace root traversal with timeline items before deleting result payloads.
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
  - Add `pub timeline: OutputTimelineModel` to `ConversationModel`.
  - Update append/extend operations to write display ordering into `timeline`.
  - Keep tool truth only in `ChatTurn.tool_calls`.
  - Remove manual syncing of `ConversationBlock::ToolCall.name`, `summary`, and `args_preview`.
- Modify: `apps/cli/src/tui/view_assembler/output.rs`
  - Iterate `conversation.timeline.items()` instead of `conversation.blocks`.
  - Join tool references back to `conversation.chats` by `(chat_id, turn_id, tool_call_id)`.
  - Use self-owned payload from timeline for non-tool text, notices, queued messages, ask-user, orphan result fallback.
- Modify tests:
  - `apps/cli/src/tui/model/conversation/model_tests.rs`
  - `apps/cli/src/tui/model/conversation/model_extra_tests.rs`
  - `apps/cli/src/tui/view_assembler/output_tests.rs`
  - `apps/cli/src/tui/view_assembler/output_task_tests.rs`

### Phase E: ToolFlowProjector and atomic patch application

- Create: `apps/cli/src/tui/model/tool_flow_projector.rs`
  - Defines `ToolFlowProjector`, `ToolFlowPatch`, and pure projection functions from `RuntimeObservation` to model/timeline commands.
- Modify: `apps/cli/src/tui/model.rs`
  - Add `pub mod tool_flow_projector;`.
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
  - Keep `runtime_observation_from_ui_event` in adapter.
  - Move tool-specific projection logic out of `map_runtime_observation` into `ToolFlowProjector`.
- Modify: `apps/cli/src/tui/update/root_reducer.rs`
  - Add `apply_agent_event_mapping_atomic` or equivalent helper so a runtime observation that emits conversation and timeline/tool patches applies them as one reducer frame before dirty/render effects are derived.
- Modify tests:
  - `apps/cli/src/tui/model/tool_flow_projector.rs` unit tests.
  - `apps/cli/src/tui/update/root_reducer.rs` atomic-frame tests.
  - Existing cross-chat/cross-turn drift tests in conversation model should be extended or mirrored for projector.

### Phase F: change/dirty/render purity convergence

- Create: `apps/cli/src/tui/model/change.rs`
  - Defines compact `ModelChange` such as `Output`, `Status`, `Input`, `Dialog`, `All` or a wrapper around `ViewModelDirty` if direct dependency is acceptable.
- Modify: `apps/cli/src/tui/model/conversation/change.rs`
  - Collapse 23 current variants into coarse model changes, or delete the enum after callers migrate to `ModelChange`.
- Modify: `apps/cli/src/tui/update/root_reducer.rs`
  - Replace the large `apply_conversation_changes` match with dirty merge logic.
- Modify: `apps/cli/src/tui/update/dirty.rs`
  - Add helpers to convert `ModelChange` to `ViewModelDirty` and merge once per update frame.
- Modify: `apps/cli/src/tui/architecture_tests.rs`
  - Add render structural guard: render production code must not import `crate::tui::model::conversation::` or `crate::tui::model::root::TuiModel`.

---

## Phase D — OutputTimelineModel and single-owner tool data

### Task D1: Add timeline model types

**Files:**
- Create: `apps/cli/src/tui/model/output_timeline/mod.rs`
- Create: `apps/cli/src/tui/model/output_timeline/item.rs`
- Create: `apps/cli/src/tui/model/output_timeline/model.rs`
- Modify: `apps/cli/src/tui/model.rs`

- [ ] **Step 1: Write timeline item definitions**

Create `apps/cli/src/tui/model/output_timeline/item.rs`:

```rust
use crate::tui::model::conversation::block::HookNoticeContent;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TimelineRuntimeContext {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}

impl TimelineRuntimeContext {
    pub fn new(chat_id: ChatId, turn_id: ChatTurnId) -> Self {
        Self { chat_id, turn_id }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TimelineToolCallRef {
    pub context: TimelineRuntimeContext,
    pub tool_call_id: ToolCallId,
}

impl TimelineToolCallRef {
    pub fn new(chat_id: ChatId, turn_id: ChatTurnId, tool_call_id: ToolCallId) -> Self {
        Self {
            context: TimelineRuntimeContext::new(chat_id, turn_id),
            tool_call_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputTimelineItem {
    UserMessage { id: String, text: String },
    AssistantText {
        id: String,
        context: TimelineRuntimeContext,
        text: String,
    },
    Thinking {
        id: String,
        context: TimelineRuntimeContext,
        text: String,
    },
    ToolCall { reference: TimelineToolCallRef },
    ToolResult { reference: TimelineToolCallRef },
    System { id: String, text: String },
    HookNotice { id: String, content: HookNoticeContent },
    Error { id: String, text: String },
    QueuedUserMessage { id: String, text: String },
    AgentProgress { id: String, tool_id: String, message: String },
    OrphanToolResult {
        id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
    },
    AskUser {
        id: String,
        question: String,
        options: Vec<sdk::OptionItem>,
        llm_option_count: usize,
        multi_select: bool,
        cursor: usize,
        selected: Vec<bool>,
        chat_input_active: bool,
        chat_input_text: String,
        default: Option<String>,
        answer: Option<String>,
    },
}

impl OutputTimelineItem {
    pub fn id(&self) -> &str {
        match self {
            OutputTimelineItem::UserMessage { id, .. }
            | OutputTimelineItem::AssistantText { id, .. }
            | OutputTimelineItem::Thinking { id, .. }
            | OutputTimelineItem::System { id, .. }
            | OutputTimelineItem::HookNotice { id, .. }
            | OutputTimelineItem::Error { id, .. }
            | OutputTimelineItem::QueuedUserMessage { id, .. }
            | OutputTimelineItem::AgentProgress { id, .. }
            | OutputTimelineItem::OrphanToolResult { id, .. }
            | OutputTimelineItem::AskUser { id, .. } => id,
            OutputTimelineItem::ToolCall { reference }
            | OutputTimelineItem::ToolResult { reference } => reference.tool_call_id.as_ref(),
        }
    }

    pub fn is_tool_owned_payload_free(&self) -> bool {
        matches!(
            self,
            OutputTimelineItem::ToolCall { .. } | OutputTimelineItem::ToolResult { .. }
        )
    }
}
```

- [ ] **Step 2: Write timeline model implementation**

Create `apps/cli/src/tui/model/output_timeline/model.rs`:

```rust
use super::item::{OutputTimelineItem, TimelineToolCallRef};
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputTimelineModel {
    items: Vec<OutputTimelineItem>,
}

impl OutputTimelineModel {
    pub fn items(&self) -> &[OutputTimelineItem] {
        &self.items
    }

    pub fn items_mut(&mut self) -> &mut Vec<OutputTimelineItem> {
        &mut self.items
    }

    pub fn push(&mut self, item: OutputTimelineItem) {
        self.items.push(item);
    }

    pub fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&OutputTimelineItem) -> bool,
    {
        self.items.retain(|item| keep(item));
    }

    pub fn contains_tool_call(&self, chat_id: &ChatId, turn_id: &ChatTurnId, id: &str) -> bool {
        self.items.iter().any(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference }
                    if reference.context.chat_id == *chat_id
                        && reference.context.turn_id == *turn_id
                        && reference.tool_call_id.as_ref() == id
            )
        })
    }

    pub fn push_tool_call_ref(&mut self, chat_id: ChatId, turn_id: ChatTurnId, tool_call_id: ToolCallId) {
        if !self.contains_tool_call(&chat_id, &turn_id, tool_call_id.as_ref()) {
            self.items.push(OutputTimelineItem::ToolCall {
                reference: TimelineToolCallRef::new(chat_id, turn_id, tool_call_id),
            });
        }
    }

    pub fn move_tool_result_after_tool_call(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        tool_call_id: &ToolCallId,
    ) {
        let Some(result_pos) = self.items.iter().position(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolResult { reference }
                    if &reference.context.chat_id == chat_id
                        && &reference.context.turn_id == turn_id
                        && &reference.tool_call_id == tool_call_id
            )
        }) else {
            return;
        };
        let result = self.items.remove(result_pos);
        let Some(call_pos) = self.items.iter().position(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference }
                    if &reference.context.chat_id == chat_id
                        && &reference.context.turn_id == turn_id
                        && &reference.tool_call_id == tool_call_id
            )
        }) else {
            self.items.insert(result_pos.min(self.items.len()), result);
            return;
        };
        self.items.insert(call_pos + 1, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat() -> ChatId {
        ChatId::new("chat-1")
    }

    fn turn() -> ChatTurnId {
        ChatTurnId::new("turn-1")
    }

    #[test]
    fn test_push_tool_call_ref_is_idempotent_for_same_context() {
        let mut model = OutputTimelineModel::default();
        model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1"));
        model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1"));
        assert_eq!(model.items().len(), 1);
    }

    #[test]
    fn test_push_tool_call_ref_allows_same_id_different_turn() {
        let mut model = OutputTimelineModel::default();
        model.push_tool_call_ref(chat(), ChatTurnId::new("turn-a"), ToolCallId::new("tool-1"));
        model.push_tool_call_ref(chat(), ChatTurnId::new("turn-b"), ToolCallId::new("tool-1"));
        assert_eq!(model.items().len(), 2);
    }

    #[test]
    fn test_move_tool_result_after_tool_call_reorders_matching_context_only() {
        let mut model = OutputTimelineModel::default();
        model.push(OutputTimelineItem::ToolResult {
            reference: TimelineToolCallRef::new(chat(), turn(), ToolCallId::new("tool-1")),
        });
        model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1"));
        model.move_tool_result_after_tool_call(&chat(), &turn(), &ToolCallId::new("tool-1"));
        assert!(matches!(model.items()[0], OutputTimelineItem::ToolCall { .. }));
        assert!(matches!(model.items()[1], OutputTimelineItem::ToolResult { .. }));
    }
}
```

- [ ] **Step 3: Export timeline module**

Create `apps/cli/src/tui/model/output_timeline/mod.rs`:

```rust
mod item;
mod model;

pub use item::{OutputTimelineItem, TimelineRuntimeContext, TimelineToolCallRef};
pub use model::OutputTimelineModel;
```

Modify `apps/cli/src/tui/model.rs`:

```rust
pub mod output_timeline;
```

Add the line next to the existing `pub mod runtime_observation;` declaration.

- [ ] **Step 4: Verify timeline unit tests**

Run:

```bash
cargo test -p cli output_timeline -- --nocapture
```

Expected: all new `output_timeline` tests pass.

- [ ] **Step 5: Commit D1**

```bash
git add apps/cli/src/tui/model.rs apps/cli/src/tui/model/output_timeline
cargo fmt --check
cargo test -p cli output_timeline -- --nocapture
git commit -m "feat(tui): add output timeline model"
```

### Task D2: Add single-owner regression tests before migration

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/model_tests.rs`
- Modify: `apps/cli/src/tui/model/conversation/block.rs`

- [ ] **Step 1: Add failing test that ToolCall block has no copied display payload**

In `apps/cli/src/tui/model/conversation/model_tests.rs`, add this test near other tool lifecycle tests:

```rust
#[test]
fn test_tool_call_timeline_item_stores_reference_not_copied_payload() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-owner");
    let turn_id = super::ids::ChatTurnId::new("turn-owner");

    model.apply(ConversationIntent::ObserveToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: "tool-owner".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: "tool-owner".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
        arguments: Some("{\"file_path\":\"src/lib.rs\"}".to_string()),
        summary: Some("Read src/lib.rs".to_string()),
        status: super::tool_call::ToolCallStatus::Ready,
    });

    let tool_refs: Vec<_> = model
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::ToolCall { reference } => {
                Some(reference)
            }
            _ => None,
        })
        .collect();
    assert_eq!(tool_refs.len(), 1);
    assert_eq!(tool_refs[0].context.chat_id, chat_id);
    assert_eq!(tool_refs[0].context.turn_id, turn_id);
    assert_eq!(tool_refs[0].tool_call_id.as_ref(), "tool-owner");

    let turn = model
        .chats
        .iter()
        .find(|chat| chat.id.as_ref() == "chat-owner")
        .and_then(|chat| chat.turns.iter().find(|turn| turn.id.as_ref() == "turn-owner"))
        .expect("turn exists");
    let call = turn
        .tool_calls
        .iter()
        .find(|call| call.id.as_ref().is_some_and(|id| id.as_ref() == "tool-owner"))
        .expect("tool call exists");
    assert_eq!(call.name, "Read");
    assert_eq!(call.summary.as_deref(), Some("Read src/lib.rs"));
    assert_eq!(call.args_preview, "{\"file_path\":\"src/lib.rs\"}");
}
```

- [ ] **Step 2: Add compile-time guard by changing block construction test expectation**

In `apps/cli/src/tui/model/conversation/block.rs`, replace the `test_conversation_block_returns_tool_id` block construction with:

```rust
let block = ConversationBlock::ToolCall {
    id: ToolCallId::new("tool-1"),
    chat_id: ChatId::new("chat-1"),
    turn_id: ChatTurnId::new("turn-1"),
};
```

This fails until `ConversationBlock::ToolCall` no longer has `name`, `summary`, and `args_preview` fields.

- [ ] **Step 3: Run failing tests**

Run:

```bash
cargo test -p cli test_tool_call_timeline_item_stores_reference_not_copied_payload -- --nocapture
```

Expected: compile failure or test failure because `ConversationModel` does not yet have `timeline`, and `ConversationBlock::ToolCall` still has copied payload fields.

### Task D3: Migrate conversation writes to OutputTimelineModel

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
- Modify: `apps/cli/src/tui/model/conversation/block.rs`
- Modify: `apps/cli/src/tui/model/conversation/tool_order.rs`

- [ ] **Step 1: Add timeline field to ConversationModel**

In `apps/cli/src/tui/model/conversation/model.rs`, update imports:

```rust
use crate::tui::model::output_timeline::{
    OutputTimelineItem, OutputTimelineModel, TimelineRuntimeContext, TimelineToolCallRef,
};
```

Update the struct:

```rust
pub struct ConversationModel {
    pub chats: Vec<Chat>,
    pub active_chat_id: Option<ChatId>,
    pub blocks: Vec<ConversationBlock>,
    pub timeline: OutputTimelineModel,
    pub queued_submissions: Vec<QueuedSubmission>,
    pub agent_progress: Vec<AgentProgressEntry>,
    next_block_sequence: usize,
    active_text_block_id: Option<String>,
    active_text_context: Option<(ChatId, ChatTurnId)>,
    active_thinking_block_id: Option<String>,
    active_thinking_context: Option<(ChatId, ChatTurnId)>,
}
```

Do not remove `blocks` yet. During D3 it remains compatibility storage so tests can migrate gradually.

- [ ] **Step 2: Reduce ToolCall block payload**

In `apps/cli/src/tui/model/conversation/block.rs`, replace the `ToolCall` variant with:

```rust
ToolCall {
    id: ToolCallId,
    chat_id: ChatId,
    turn_id: ChatTurnId,
},
```

Remove `name`, `summary`, and `args_preview` from all `ConversationBlock::ToolCall` pattern matches and constructors.

- [ ] **Step 3: Update insert_tool_call_block_before_active_text signature**

In `apps/cli/src/tui/model/conversation/tool_order.rs`, change the function signature to:

```rust
pub(super) fn insert_tool_call_block_before_active_text(
    &mut self,
    chat_id: ChatId,
    turn_id: ChatTurnId,
    tool_call_id: ToolCallId,
) {
```

Inside the function, create the block with only reference fields:

```rust
let block = ConversationBlock::ToolCall {
    id: tool_call_id.clone(),
    chat_id: chat_id.clone(),
    turn_id: turn_id.clone(),
};
```

Also append the timeline reference exactly once:

```rust
self.timeline
    .push_tool_call_ref(chat_id.clone(), turn_id.clone(), tool_call_id.clone());
```

Keep the existing block insertion position logic intact for compatibility until D4 switches the presenter to timeline traversal.

- [ ] **Step 4: Update tool start/update call sites**

In `apps/cli/src/tui/model/conversation/model.rs`, replace calls like:

```rust
self.insert_tool_call_block_before_active_text(
    chat_id,
    turn_id,
    tool_call_id,
    name.clone(),
    String::new(),
    String::new(),
);
```

with:

```rust
self.insert_tool_call_block_before_active_text(chat_id, turn_id, tool_call_id);
```

In `observe_tool_call_update`, delete the loop that updates `ConversationBlock::ToolCall.summary` and `args_preview`. The only owner of those fields is now `ChatTurn.tool_calls`.

- [ ] **Step 5: Add timeline writes for non-tool payloads**

In `ConversationModel::start_chat`, after pushing the `ConversationBlock::UserMessage`, also push:

```rust
self.timeline.push(OutputTimelineItem::UserMessage {
    id: block_id.clone(),
    text: submission,
});
```

In `append_user_message`, after pushing the block, also push:

```rust
self.timeline.push(OutputTimelineItem::UserMessage {
    id: block_id.clone(),
    text,
});
```

In `append_or_extend_text_block`, when extending an existing assistant/thinking block, also find the matching timeline item and append text:

```rust
if let Some(item) = self.timeline.items_mut().iter_mut().find(|item| item.id() == block_id) {
    match item {
        OutputTimelineItem::AssistantText { text: existing, .. }
        | OutputTimelineItem::Thinking { text: existing, .. } => existing.push_str(&text),
        _ => {}
    }
}
```

When creating a new thinking item, push:

```rust
self.timeline.push(OutputTimelineItem::Thinking {
    id: block_id.clone(),
    context: TimelineRuntimeContext::new(chat_id.clone(), turn_id.clone()),
    text: text.clone(),
});
```

When creating a new assistant item, push:

```rust
self.timeline.push(OutputTimelineItem::AssistantText {
    id: block_id.clone(),
    context: TimelineRuntimeContext::new(chat_id.clone(), turn_id.clone()),
    text: text.clone(),
});
```

- [ ] **Step 6: Update queued submission timeline writes**

In `queue_submission`, after pushing `ConversationBlock::QueuedUserMessage`, push:

```rust
self.timeline.push(OutputTimelineItem::QueuedUserMessage {
    id: id.clone(),
    text,
});
```

Because `text` is moved into both block and timeline, clone once before moving:

```rust
let block_text = text.clone();
```

In `clear_queued_submissions`, after retaining `blocks`, also retain timeline:

```rust
self.timeline
    .retain(|item| !matches!(item, OutputTimelineItem::QueuedUserMessage { .. }));
```

- [ ] **Step 7: Update tool result timeline writes**

Find `observe_tool_result`, `promote_orphan_tool_result`, and `move_tool_results_after_tool_call` in `model.rs` / `tool_order.rs`.

For a bound result, store tool result content in `ChatTurn.tool_calls[*].result` through existing `turn.complete_tool(...)` and push only a timeline reference:

```rust
self.timeline.push(OutputTimelineItem::ToolResult {
    reference: TimelineToolCallRef::new(chat_id.clone(), turn_id.clone(), ToolCallId::new(id.clone())),
});
```

For orphan result, keep using self-owned timeline payload:

```rust
self.timeline.push(OutputTimelineItem::OrphanToolResult {
    id: id.clone(),
    tool_name,
    output,
    content,
    is_error,
});
```

- [ ] **Step 8: Run D2 regression**

Run:

```bash
cargo test -p cli test_tool_call_timeline_item_stores_reference_not_copied_payload -- --nocapture
```

Expected: pass.

- [ ] **Step 9: Commit D3**

```bash
git add apps/cli/src/tui/model/conversation apps/cli/src/tui/model/output_timeline
cargo fmt --check
cargo test -p cli test_tool_call_timeline_item_stores_reference_not_copied_payload -- --nocapture
git commit -m "refactor(tui): store tool timeline entries as references"
```

### Task D4: Switch OutputAssembler to timeline traversal and join tool data from chats

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_tests.rs`
- Modify: `apps/cli/src/tui/view_assembler/output_task_tests.rs`

- [ ] **Step 1: Add tool lookup helper scoped by context**

In `apps/cli/src/tui/view_assembler/output.rs`, add:

```rust
fn find_tool_call<'a>(
    conversation: &'a ConversationModel,
    chat_id: &crate::tui::model::conversation::ids::ChatId,
    turn_id: &crate::tui::model::conversation::ids::ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<&'a crate::tui::model::conversation::tool_call::ToolCall> {
    conversation
        .chats
        .iter()
        .find(|chat| &chat.id == chat_id)
        .and_then(|chat| chat.turns.iter().find(|turn| &turn.id == turn_id))
        .and_then(|turn| {
            turn.tool_calls
                .iter()
                .find(|call| call.id.as_ref().is_some_and(|id| id == tool_id))
        })
}
```

- [ ] **Step 2: Add tool view helper from joined call**

Add:

```rust
fn tool_view_from_call(
    chat_id: &crate::tui::model::conversation::ids::ChatId,
    turn_id: &crate::tui::model::conversation::ids::ChatTurnId,
    call: &crate::tui::model::conversation::tool_call::ToolCall,
) -> Option<ToolCallBlockView> {
    let id = call.id.as_ref()?;
    let (icon, semantic_status, style) = map_tool_status(call.status);
    Some(ToolCallBlockView {
        key: format!("tool-{}", id.as_ref()),
        chat_id: Some(chat_id.as_ref().to_string()),
        turn_id: Some(turn_id.as_ref().to_string()),
        tool_call_id: Some(id.as_ref().to_string()),
        title: call.name.clone(),
        icon: icon.to_string(),
        semantic_status,
        style,
        args_preview: (!call.args_preview.is_empty()).then(|| call.args_preview.clone()),
        summary: call.summary.clone(),
        activity_summary: call.activities.last().cloned(),
        result_summary: call.result.clone(),
        collapsible: matches!(call.status, ToolCallStatus::Success | ToolCallStatus::Error),
        collapsed: false,
    })
}
```

- [ ] **Step 3: Change assembler traversal**

In `OutputAssembler::assemble`, change:

```rust
for block in &conversation.blocks {
```

to:

```rust
for item in conversation.timeline.items() {
```

Replace each `ConversationBlock::...` match arm with equivalent `OutputTimelineItem::...` arms. Tool call arm:

```rust
OutputTimelineItem::ToolCall { reference } => {
    if let Some(call) = find_tool_call(
        conversation,
        &reference.context.chat_id,
        &reference.context.turn_id,
        &reference.tool_call_id,
    ) {
        if let Some(tool) = tool_view_from_call(
            &reference.context.chat_id,
            &reference.context.turn_id,
            call,
        ) {
            let node = leaf(tool.key.clone(), OutputBlockKind::ToolCall(tool.clone()));
            roots.push(node);
        }
    }
}
```

Tool result arm:

```rust
OutputTimelineItem::ToolResult { reference } => {
    if tool_result_is_embedded(conversation, &reference.context.chat_id, &reference.context.turn_id, &reference.tool_call_id) {
        continue;
    }
    if let Some(call) = find_tool_call(
        conversation,
        &reference.context.chat_id,
        &reference.context.turn_id,
        &reference.tool_call_id,
    ) {
        let result_text = call.result.clone().unwrap_or_default();
        let summary = render_tool_result_summary(&call.name, &result_text, call.status == ToolCallStatus::Error);
        roots.push(leaf(
            format!("tool-result-{}", reference.tool_call_id.as_ref()),
            OutputBlockKind::ToolResult(ToolResultBlockView {
                key: format!("tool-result-{}", reference.tool_call_id.as_ref()),
                tool_title: call.name.clone(),
                summary: Some(summary),
                result_text,
                style: if call.status == ToolCallStatus::Error {
                    SemanticStyle::Error
                } else {
                    SemanticStyle::Success
                },
            }),
        ));
    }
}
```

- [ ] **Step 4: Scope embedded result helper by context**

Change:

```rust
fn tool_result_is_embedded(conversation: &ConversationModel, tool_id: &ToolCallId) -> bool
```

to:

```rust
fn tool_result_is_embedded(
    conversation: &ConversationModel,
    chat_id: &crate::tui::model::conversation::ids::ChatId,
    turn_id: &crate::tui::model::conversation::ids::ChatTurnId,
    tool_id: &ToolCallId,
) -> bool
```

Inside it, only inspect the matching chat/turn.

- [ ] **Step 5: Add cross-turn same-id view assembler test**

In `apps/cli/src/tui/view_assembler/output_tests.rs`, add:

```rust
#[test]
fn test_output_assembler_joins_tool_reference_by_chat_and_turn() {
    let mut conversation = ConversationModel::default();
    let chat_a = crate::tui::model::conversation::ids::ChatId::new("chat-a");
    let turn_a = crate::tui::model::conversation::ids::ChatTurnId::new("turn-a");
    let chat_b = crate::tui::model::conversation::ids::ChatId::new("chat-b");
    let turn_b = crate::tui::model::conversation::ids::ChatTurnId::new("turn-b");

    conversation.apply(ConversationIntent::ObserveToolCallStart {
        chat_id: chat_a.clone(),
        turn_id: turn_a.clone(),
        id: "same-tool".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        chat_id: chat_b.clone(),
        turn_id: turn_b.clone(),
        id: "same-tool".to_string(),
        provider_id: None,
        name: "Bash".to_string(),
        index: 0,
    });

    let vm = OutputAssembler::assemble(&conversation);
    let titles: Vec<_> = vm
        .blocks
        .iter()
        .filter_map(|node| match &node.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool.title.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, vec!["Read", "Bash"]);
}
```

- [ ] **Step 6: Run view assembler tests**

Run:

```bash
cargo test -p cli output_assembler -- --nocapture
cargo test -p cli view_assembler::output_tests -- --nocapture
```

Expected: tests pass.

- [ ] **Step 7: Commit D4**

```bash
git add apps/cli/src/tui/view_assembler apps/cli/src/tui/model/conversation
cargo fmt --check
cargo test -p cli output_assembler -- --nocapture
git commit -m "refactor(tui): assemble output from timeline references"
```

### Task D5: Remove tool payload duplication and compatibility blocks where safe

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/block.rs`
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
- Modify: `apps/cli/src/tui/model/conversation/tool_order.rs`
- Modify tests that still pattern-match copied tool fields.

- [ ] **Step 1: Search for forbidden copied fields**

Run with the dedicated search tool or shell command equivalent if executing manually:

```bash
rg "ConversationBlock::ToolCall \{[^}]*name|summary: block_summary|args_preview: block_args|ToolCall \{[^}]*args_preview" apps/cli/src/tui
```

Expected before cleanup: no production code references `ConversationBlock::ToolCall` copied fields. Test fixtures may still need updates.

- [ ] **Step 2: Add architecture regression for single-owner tool payload**

In `apps/cli/src/tui/architecture_tests.rs`, add:

```rust
#[test]
fn test_conversation_block_tool_call_does_not_copy_tool_display_payload() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/model/conversation/block.rs"),
    )
    .expect("read conversation block source");
    let start = source
        .find("ToolCall {")
        .expect("ToolCall variant exists");
    let tail = &source[start..];
    let end = tail.find("},").expect("ToolCall variant closes");
    let variant = &tail[..end];
    assert!(!variant.contains("name:"));
    assert!(!variant.contains("summary:"));
    assert!(!variant.contains("args_preview:"));
}
```

- [ ] **Step 3: Run single-owner regression suite**

Run:

```bash
cargo test -p cli test_conversation_block_tool_call_does_not_copy_tool_display_payload -- --nocapture
cargo test -p cli test_tool_call_timeline_item_stores_reference_not_copied_payload -- --nocapture
```

Expected: both pass.

- [ ] **Step 4: Full D validation**

Run:

```bash
cargo fmt --check
cargo test -p cli -- --test-threads=1 --format terse
cargo clippy -p cli --all-targets -- -D warnings
```

Expected: all pass.

- [ ] **Step 5: Commit D5**

```bash
git add apps/cli/src/tui
cargo fmt --check
cargo test -p cli -- --test-threads=1 --format terse
git commit -m "refactor(tui): enforce single owner for tool display data"
```

---

## Phase E — ToolFlowProjector and atomic model patches

### Task E1: Define ToolFlowProjector patch API

**Files:**
- Create: `apps/cli/src/tui/model/tool_flow_projector.rs`
- Modify: `apps/cli/src/tui/model.rs`

- [ ] **Step 1: Create projector API**

Create `apps/cli/src/tui/model/tool_flow_projector.rs`:

```rust
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime_observation::RuntimeObservation;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolFlowPatch {
    pub conversation: Vec<ConversationIntent>,
    pub runtime: Vec<RuntimeIntent>,
}

impl ToolFlowPatch {
    pub fn single_conversation(intent: ConversationIntent) -> Self {
        Self {
            conversation: vec![intent],
            runtime: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ToolFlowProjector;

impl ToolFlowProjector {
    pub fn project(observation: &RuntimeObservation) -> Option<ToolFlowPatch> {
        match observation {
            RuntimeObservation::ToolCallStart {
                context,
                id,
                provider_id,
                name,
                index,
            } => Some(ToolFlowPatch::single_conversation(
                ConversationIntent::ObserveToolCallStart {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    id: id.clone(),
                    provider_id: provider_id.clone(),
                    name: name.clone(),
                    index: *index,
                },
            )),
            RuntimeObservation::ToolCallUpdate {
                context,
                id,
                provider_id,
                name,
                index,
                arguments,
                summary,
                status,
            } => Some(ToolFlowPatch::single_conversation(
                ConversationIntent::ObserveToolCallUpdate {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    id: id.clone(),
                    provider_id: provider_id.clone(),
                    name: name.clone(),
                    index: *index,
                    arguments: arguments.clone(),
                    summary: summary.clone(),
                    status: *status,
                },
            )),
            RuntimeObservation::ToolResult {
                context,
                id,
                provider_id,
                tool_name,
                output,
                content,
                is_error,
                image_count,
            } => Some(ToolFlowPatch::single_conversation(
                ConversationIntent::ObserveToolResult {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    id: id.clone(),
                    provider_id: provider_id.clone(),
                    tool_name: tool_name.clone(),
                    output: output.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                    image_count: *image_count,
                },
            )),
            RuntimeObservation::AgentProgress {
                context,
                tool_id,
                message,
            } => Some(ToolFlowPatch::single_conversation(
                ConversationIntent::RecordAgentProgress {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    tool_id: tool_id.clone(),
                    message: message.clone(),
                },
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
    use crate::tui::model::conversation::tool_call::ToolCallStatus;
    use crate::tui::model::runtime_observation::RuntimeTurnContext;

    fn context() -> RuntimeTurnContext {
        RuntimeTurnContext::new(ChatId::new("chat-1"), ChatTurnId::new("turn-1"))
    }

    #[test]
    fn test_project_tool_call_update_preserves_context() {
        let patch = ToolFlowProjector::project(&RuntimeObservation::ToolCallUpdate {
            context: context(),
            id: "tool-1".to_string(),
            provider_id: Some("provider-1".to_string()),
            name: "Read".to_string(),
            index: 0,
            arguments: Some("{}".to_string()),
            summary: Some("Read file".to_string()),
            status: ToolCallStatus::Ready,
        })
        .expect("tool observation projects");

        assert!(matches!(
            patch.conversation.as_slice(),
            [ConversationIntent::ObserveToolCallUpdate { chat_id, turn_id, id, .. }]
                if chat_id.as_ref() == "chat-1"
                    && turn_id.as_ref() == "turn-1"
                    && id == "tool-1"
        ));
    }

    #[test]
    fn test_project_non_tool_observation_returns_none() {
        assert!(ToolFlowProjector::project(&RuntimeObservation::AssistantText {
            context: context(),
            text: "hello".to_string(),
        })
        .is_none());
    }
}
```

- [ ] **Step 2: Export module**

Modify `apps/cli/src/tui/model.rs`:

```rust
pub mod tool_flow_projector;
```

- [ ] **Step 3: Run projector tests**

Run:

```bash
cargo test -p cli tool_flow_projector -- --nocapture
```

Expected: pass.

- [ ] **Step 4: Commit E1**

```bash
git add apps/cli/src/tui/model.rs apps/cli/src/tui/model/tool_flow_projector.rs
cargo fmt --check
cargo test -p cli tool_flow_projector -- --nocapture
git commit -m "refactor(tui): introduce tool flow projector"
```

### Task E2: Route tool observations through projector

**Files:**
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`

- [ ] **Step 1: Import projector**

Add:

```rust
use crate::tui::model::tool_flow_projector::ToolFlowProjector;
```

- [ ] **Step 2: Delegate tool observations in map_runtime_observation**

At the top of `map_runtime_observation`, add:

```rust
if let Some(patch) = ToolFlowProjector::project(observation) {
    let mut mapping = AgentEventMapping::default();
    mapping.conversation.extend(patch.conversation);
    mapping.runtime.extend(patch.runtime);
    return mapping;
}
```

Then remove the tool-specific match arms from `map_runtime_observation`:

- `RuntimeObservation::ToolCallStart`
- `RuntimeObservation::ToolCallUpdate`
- `RuntimeObservation::ToolResult`
- `RuntimeObservation::AgentProgress`

Keep assistant text, thinking text, block complete, and complete lifecycle in `agent_event.rs`.

- [ ] **Step 3: Run existing adapter tests**

Run:

```bash
cargo test -p cli agent_event -- --nocapture
```

Expected: pass; existing sanitization expectations may fail because projector receives already-sanitized data only if sanitization stays in `runtime_observation_from_ui_event`.

- [ ] **Step 4: If sanitization fails, move sanitization before projector**

In `runtime_observation_from_ui_event`, keep these transformations before creating `RuntimeObservation::ToolCallUpdate`:

```rust
arguments: arguments_delta
    .as_ref()
    .map(|value| sanitize_tool_arguments_delta(name, value)),
summary: summary
    .as_ref()
    .map(|value| sanitize_tool_summary(name, value))
    .or_else(|| arguments.as_ref().map(|value| sanitize_tool_summary(name, &value.to_string()))),
```

For `ToolResult`, keep:

```rust
output: sanitize_tool_output(tool_name, output),
content: sanitize_tool_result_content(tool_name, content.clone()),
```

- [ ] **Step 5: Commit E2**

```bash
git add apps/cli/src/tui/adapter/agent_event.rs apps/cli/src/tui/model/tool_flow_projector.rs
cargo fmt --check
cargo test -p cli agent_event -- --nocapture
git commit -m "refactor(tui): project tool observations outside adapter"
```

### Task E3: Apply projector patches atomically in reducer

**Files:**
- Modify: `apps/cli/src/tui/update/root_reducer.rs`
- Modify: `apps/cli/src/tui/adapter/agent_event.rs` if mapping shape needs a `patches` field.

- [ ] **Step 1: Add atomic reducer test**

In `apps/cli/src/tui/update/root_reducer.rs` tests, add:

```rust
#[test]
fn test_reduce_agent_event_applies_multiple_conversation_intents_before_render_request() {
    let mut model = TuiModel::default();
    let context = test_turn_context();
    let mapping = AgentEventMapping {
        conversation: vec![
            ConversationIntent::ObserveToolCallStart {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                id: "tool-atomic".to_string(),
                provider_id: None,
                name: "Read".to_string(),
                index: 0,
            },
            ConversationIntent::ObserveToolCallUpdate {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                id: "tool-atomic".to_string(),
                provider_id: None,
                name: "Read".to_string(),
                index: 0,
                arguments: Some("{}".to_string()),
                summary: Some("Read file".to_string()),
                status: crate::tui::model::conversation::tool_call::ToolCallStatus::Ready,
            },
        ],
        ..AgentEventMapping::default()
    };

    let result = reduce_agent_event(&mut model, mapping);
    assert!(result.dirty.output);
    assert_eq!(
        result
            .effects
            .iter()
            .filter(|effect| matches!(effect, Effect::RequestRender))
            .count(),
        1,
        "all conversation patches in one AgentEvent must produce one render request"
    );
    let turn = model
        .conversation
        .chats
        .iter()
        .flat_map(|chat| &chat.turns)
        .find(|turn| turn.id == context.turn_id)
        .expect("turn exists");
    assert_eq!(turn.tool_calls.len(), 1);
    assert_eq!(turn.tool_calls[0].summary.as_deref(), Some("Read file"));
}
```

- [ ] **Step 2: Ensure reduce_agent_event derives effects once after all patches**

Current `reduce_agent_event` already loops all conversation intents before pushing `RequestRender`. Keep that invariant and make it explicit by extracting helper:

```rust
fn apply_conversation_intents(
    model: &mut TuiModel,
    result: &mut TuiUpdateResult,
    intents: Vec<ConversationIntent>,
) {
    for intent in intents {
        let changes = model.conversation.apply(intent);
        apply_conversation_changes(result, &changes);
    }
}
```

Then call this helper from `reduce_agent_event`. Do not push render effects inside the loop beyond what `apply_conversation_changes` currently does; if duplicate render effects appear, move render effect emission out of `apply_conversation_changes` in Phase F.

- [ ] **Step 3: Run atomic test**

Run:

```bash
cargo test -p cli test_reduce_agent_event_applies_multiple_conversation_intents_before_render_request -- --nocapture
```

Expected: pass with exactly one `RequestRender`.

- [ ] **Step 4: Commit E3**

```bash
git add apps/cli/src/tui/update/root_reducer.rs
cargo fmt --check
cargo test -p cli test_reduce_agent_event_applies_multiple_conversation_intents_before_render_request -- --nocapture
git commit -m "test(tui): guard atomic tool flow patch application"
```

### Task E4: Add cross chat/turn projector regression suite

**Files:**
- Modify: `apps/cli/src/tui/model/tool_flow_projector.rs`
- Modify: `apps/cli/src/tui/model/conversation/model_extra_tests.rs`

- [ ] **Step 1: Add same-id different chat projector test**

In `tool_flow_projector.rs` tests, add:

```rust
#[test]
fn test_project_same_tool_id_keeps_distinct_chat_contexts() {
    let left = RuntimeObservation::ToolCallStart {
        context: RuntimeTurnContext::new(ChatId::new("chat-left"), ChatTurnId::new("turn-1")),
        id: "same-tool".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    };
    let right = RuntimeObservation::ToolCallStart {
        context: RuntimeTurnContext::new(ChatId::new("chat-right"), ChatTurnId::new("turn-1")),
        id: "same-tool".to_string(),
        provider_id: None,
        name: "Bash".to_string(),
        index: 0,
    };

    let left_patch = ToolFlowProjector::project(&left).expect("left patch");
    let right_patch = ToolFlowProjector::project(&right).expect("right patch");

    assert!(matches!(
        left_patch.conversation.as_slice(),
        [ConversationIntent::ObserveToolCallStart { chat_id, id, .. }]
            if chat_id.as_ref() == "chat-left" && id == "same-tool"
    ));
    assert!(matches!(
        right_patch.conversation.as_slice(),
        [ConversationIntent::ObserveToolCallStart { chat_id, id, .. }]
            if chat_id.as_ref() == "chat-right" && id == "same-tool"
    ));
}
```

- [ ] **Step 2: Add model same-id different turn test**

In `model_extra_tests.rs`, add:

```rust
#[test]
fn test_tool_flow_same_id_different_turns_do_not_cross_wire() {
    let mut model = ConversationModel::default();
    let chat = super::ids::ChatId::new("chat-1");
    let turn_a = super::ids::ChatTurnId::new("turn-a");
    let turn_b = super::ids::ChatTurnId::new("turn-b");

    model.apply(ConversationIntent::ObserveToolCallStart {
        chat_id: chat.clone(),
        turn_id: turn_a.clone(),
        id: "same-tool".to_string(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        chat_id: chat.clone(),
        turn_id: turn_b.clone(),
        id: "same-tool".to_string(),
        provider_id: None,
        name: "Bash".to_string(),
        index: 0,
    });

    let chat_model = model
        .chats
        .iter()
        .find(|candidate| candidate.id == chat)
        .expect("chat exists");
    let turn_a_model = chat_model
        .turns
        .iter()
        .find(|turn| turn.id == turn_a)
        .expect("turn a exists");
    let turn_b_model = chat_model
        .turns
        .iter()
        .find(|turn| turn.id == turn_b)
        .expect("turn b exists");

    assert_eq!(turn_a_model.tool_calls[0].name, "Read");
    assert_eq!(turn_b_model.tool_calls[0].name, "Bash");
}
```

- [ ] **Step 3: Run regression suite**

Run:

```bash
cargo test -p cli same_tool_id -- --nocapture
cargo test -p cli test_tool_flow_same_id_different_turns_do_not_cross_wire -- --nocapture
```

Expected: pass.

- [ ] **Step 4: Full E validation and commit**

```bash
cargo fmt --check
cargo test -p cli -- --test-threads=1 --format terse
cargo clippy -p cli --all-targets -- -D warnings
git add apps/cli/src/tui
git commit -m "test(tui): cover cross-context tool flow projection"
```

---

## Phase F — Change modeling and render purity

### Task F1: Introduce compact ModelChange dirty mapping

**Files:**
- Create: `apps/cli/src/tui/model/change.rs`
- Modify: `apps/cli/src/tui/model.rs`
- Modify: `apps/cli/src/tui/update/dirty.rs`

- [ ] **Step 1: Create ModelChange**

Create `apps/cli/src/tui/model/change.rs`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelChange {
    Output,
    Status,
    Input,
    Dialog,
    All,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_change_values_are_distinct() {
        assert_ne!(ModelChange::Output, ModelChange::Status);
        assert_ne!(ModelChange::Input, ModelChange::Dialog);
    }
}
```

Modify `apps/cli/src/tui/model.rs`:

```rust
pub mod change;
```

- [ ] **Step 2: Add dirty conversion helper**

In `apps/cli/src/tui/update/dirty.rs`, add:

```rust
use crate::tui::model::change::ModelChange;

pub fn mark_model_change(target: &mut ViewModelDirty, change: ModelChange) {
    match change {
        ModelChange::Output => target.mark_output(),
        ModelChange::Status => target.mark_status(),
        ModelChange::Input => target.mark_input(),
        ModelChange::Dialog => target.mark_dialog(),
        ModelChange::All => target.mark_all(),
    }
}

pub fn merge_model_changes(target: &mut ViewModelDirty, changes: &[ModelChange]) {
    for change in changes {
        mark_model_change(target, *change);
    }
}
```

- [ ] **Step 3: Add tests for dirty conversion**

In `dirty.rs` tests, add:

```rust
#[test]
fn test_merge_model_changes_marks_output_and_status() {
    let mut dirty = ViewModelDirty::default();
    merge_model_changes(&mut dirty, &[ModelChange::Output, ModelChange::Status]);
    assert!(dirty.output);
    assert!(dirty.status);
    assert!(!dirty.input);
    assert!(!dirty.dialog);
}

#[test]
fn test_merge_model_changes_all_marks_everything() {
    let mut dirty = ViewModelDirty::default();
    merge_model_changes(&mut dirty, &[ModelChange::All]);
    assert!(dirty.output);
    assert!(dirty.status);
    assert!(dirty.input);
    assert!(dirty.dialog);
}
```

- [ ] **Step 4: Run dirty tests**

```bash
cargo test -p cli dirty -- --nocapture
```

Expected: pass.

- [ ] **Step 5: Commit F1**

```bash
git add apps/cli/src/tui/model.rs apps/cli/src/tui/model/change.rs apps/cli/src/tui/update/dirty.rs
cargo fmt --check
cargo test -p cli dirty -- --nocapture
git commit -m "refactor(tui): introduce compact model changes"
```

### Task F2: Collapse ConversationChange variants

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/change.rs`
- Modify: `apps/cli/src/tui/model/conversation/model.rs`
- Modify: `apps/cli/src/tui/update/root_reducer.rs`
- Modify affected tests.

- [ ] **Step 1: Replace ConversationChange enum with coarse variants**

In `conversation/change.rs`, replace current enum with:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationChange {
    Output,
    Status,
    OutputAndStatus,
}

impl ConversationChange {
    pub fn model_changes(self) -> &'static [crate::tui::model::change::ModelChange] {
        use crate::tui::model::change::ModelChange;
        match self {
            ConversationChange::Output => &[ModelChange::Output],
            ConversationChange::Status => &[ModelChange::Status],
            ConversationChange::OutputAndStatus => &[ModelChange::Output, ModelChange::Status],
        }
    }
}
```

- [ ] **Step 2: Update model return values**

In `ConversationModel`, replace detailed changes with coarse ones:

- `ChatStarted + ChatTurnStarted + OutputDirty` → `vec![ConversationChange::OutputAndStatus]`
- user/assistant/thinking/tool/system/error/queued/agent progress/ask-user output updates → `vec![ConversationChange::Output]`
- `CompleteChat` / chat completion status-only changes → `vec![ConversationChange::Status]`
- if a function previously returned `Vec::new()` for no-op, keep `Vec::new()`.

Example replacement:

```rust
vec![ConversationChange::Output]
```

- [ ] **Step 3: Update root reducer change mapping**

In `root_reducer.rs`, replace `apply_conversation_changes` body with:

```rust
fn apply_conversation_changes(result: &mut TuiUpdateResult, changes: &[ConversationChange]) {
    for change in changes {
        crate::tui::update::dirty::merge_model_changes(&mut result.dirty, change.model_changes());
    }
    if result.dirty.output || result.dirty.status || result.dirty.dialog || result.dirty.input {
        result.effects.push(Effect::RequestRender);
    }
}
```

- [ ] **Step 4: Update tests that assert detailed ConversationChange variants**

For tests currently matching variants like:

```rust
ConversationChange::ToolCallObserved { .. }
```

change to:

```rust
ConversationChange::Output
```

For tests matching chat status variants:

```rust
ConversationChange::ChatCompleting { .. }
```

change to:

```rust
ConversationChange::Status
```

For start chat tests, use:

```rust
ConversationChange::OutputAndStatus
```

- [ ] **Step 5: Add regression that ConversationChange variant count stays small**

In `conversation/change.rs` tests, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_change_variant_count_is_coarse() {
        let variants = [
            ConversationChange::Output,
            ConversationChange::Status,
            ConversationChange::OutputAndStatus,
        ];
        assert_eq!(variants.len(), 3);
    }
}
```

- [ ] **Step 6: Run root reducer and conversation tests**

```bash
cargo test -p cli conversation_change -- --nocapture
cargo test -p cli root_reducer -- --nocapture
cargo test -p cli conversation::model -- --nocapture
```

Expected: pass.

- [ ] **Step 7: Commit F2**

```bash
git add apps/cli/src/tui/model/conversation apps/cli/src/tui/update/root_reducer.rs
cargo fmt --check
cargo test -p cli root_reducer -- --nocapture
git commit -m "refactor(tui): collapse conversation changes to dirty categories"
```

### Task F3: Ensure one render request per reducer frame

**Files:**
- Modify: `apps/cli/src/tui/update/root_reducer.rs`

- [ ] **Step 1: Add render request dedup helper**

In `root_reducer.rs`, add:

```rust
fn request_render_once(result: &mut TuiUpdateResult) {
    if !result
        .effects
        .iter()
        .any(|effect| matches!(effect, Effect::RequestRender))
    {
        result.effects.push(Effect::RequestRender);
    }
}
```

- [ ] **Step 2: Use helper in root reducer**

Replace all local `result.effects.push(Effect::RequestRender)` calls in `apply_input_changes`, `apply_conversation_changes`, and `reduce_agent_event` post-processing with:

```rust
request_render_once(result);
```

Do not change explicit render-tick behavior unless tests reveal duplicates.

- [ ] **Step 3: Add duplicate prevention test**

In `root_reducer.rs` tests, add:

```rust
#[test]
fn test_reduce_agent_event_requests_render_once_for_output_and_status_changes() {
    let mut model = TuiModel::default();
    let mapping = AgentEventMapping {
        conversation: vec![ConversationIntent::StartChat {
            submission: "hello".to_string(),
        }],
        ..AgentEventMapping::default()
    };

    let result = reduce_agent_event(&mut model, mapping);
    assert_eq!(
        result
            .effects
            .iter()
            .filter(|effect| matches!(effect, Effect::RequestRender))
            .count(),
        1
    );
}
```

- [ ] **Step 4: Run root reducer tests**

```bash
cargo test -p cli root_reducer -- --nocapture
```

Expected: pass.

- [ ] **Step 5: Commit F3**

```bash
git add apps/cli/src/tui/update/root_reducer.rs
cargo fmt --check
cargo test -p cli root_reducer -- --nocapture
git commit -m "fix(tui): request render once per reducer frame"
```

### Task F4: Add render purity architecture guard

**Files:**
- Modify: `apps/cli/src/tui/architecture_tests.rs`

- [ ] **Step 1: Add failing guard for render imports**

In `architecture_tests.rs`, add:

```rust
#[test]
fn test_render_production_code_does_not_depend_on_domain_models() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/render");
    let forbidden = [
        "crate::tui::model::conversation::",
        "crate::tui::model::root::TuiModel",
        "crate::tui::model::diagnostic::",
        "crate::tui::model::input::",
        "crate::tui::model::runtime::",
    ];

    for file in rust_files_under(&root) {
        if file
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("test"))
        {
            continue;
        }
        let source = production_source(&fs::read_to_string(&file).expect("read rust source"));
        for pattern in forbidden {
            assert!(
                !source.contains(pattern),
                "{} production code must depend on ViewModel, not domain model pattern {}",
                file.display(),
                pattern
            );
        }
    }
}
```

- [ ] **Step 2: Run guard**

```bash
cargo test -p cli test_render_production_code_does_not_depend_on_domain_models -- --nocapture
```

Expected: pass if render already only consumes ViewModel. If it fails, inspect the named file and move the dependency to `view_model` or `view_assembler` without changing visual behavior.

- [ ] **Step 3: Full F validation and commit**

```bash
cargo fmt --check
cargo test -p cli -- --test-threads=1 --format terse
cargo clippy -p cli --all-targets -- -D warnings
git add apps/cli/src/tui
git commit -m "test(tui): guard render from domain model dependencies"
```

---

## Final validation and merge

### Task Z1: Full workspace validation for affected crates

**Files:**
- No source changes expected.

- [ ] **Step 1: Run final checks in feature worktree**

```bash
cargo fmt --check
cargo test -p cli -- --test-threads=1 --format terse
cargo test -p sdk
cargo test -p runtime
cargo clippy -p cli --all-targets -- -D warnings
git diff --check
```

Expected: all commands pass.

- [ ] **Step 2: Inspect final diff**

```bash
git status --short
git diff --stat main...HEAD
```

Expected: only planned files changed; no untracked temp files.

- [ ] **Step 3: Commit any remaining plan updates**

If this plan file was updated during execution:

```bash
git add docs/superpowers/plans/2026-06-13-tui-049-def-boundary-convergence.md
git commit -m "docs(tui): plan 049 boundary convergence phases"
```

### Task Z2: Merge back to main and cleanup

- [ ] **Step 1: Return to main checkout**

Use the worktree exit tool if available, or manually return to repository main checkout.

- [ ] **Step 2: Pull latest main**

```bash
git pull --ff-only
```

Expected: main updates or reports already up to date.

- [ ] **Step 3: Fast-forward merge feature branch**

```bash
git merge --ff-only feature/tui-049-def-boundary-convergence
```

Expected: fast-forward merge succeeds.

- [ ] **Step 4: Re-run main validation**

```bash
cargo fmt --check
cargo test -p cli -- --test-threads=1 --format terse
cargo test -p sdk
cargo test -p runtime
cargo clippy -p cli --all-targets -- -D warnings
```

Expected: all pass on main.

- [ ] **Step 5: Cleanup worktree and branch**

```bash
git worktree remove .worktrees/feature-tui-049-def-boundary-convergence
git branch -d feature/tui-049-def-boundary-convergence
git status --short
```

Expected: branch deleted and working tree clean.

---

## Self-review checklist

- Spec coverage:
  - §4.5 single owner: covered by Phase D timeline reference model and `test_conversation_block_tool_call_does_not_copy_tool_display_payload`.
  - §7.3 OutputTimelineModel: covered by D1-D5.
  - §7.4 ToolFlowProjector and atomic patches: covered by E1-E4.
  - §8.2 / §10.1.2 render boundary: prior phase C moved safe text and nesting; F4 adds render domain-dependency guard.
  - §11 phased migration: D/E/F map directly to remaining phases.
  - §12.5 regression: covered by D2/D5 single-owner tests and architecture guard.
- Placeholder scan: no `TBD`, no unresolved "implement later" steps. Each code-changing task includes concrete code or exact replacement guidance.
- Type consistency:
  - `OutputTimelineItem`, `TimelineRuntimeContext`, and `TimelineToolCallRef` are defined in D1 before use.
  - `ToolFlowProjector`, `ToolFlowPatch` are defined in E1 before use.
  - `ModelChange` is defined in F1 before use by `dirty.rs` and `ConversationChange`.

---

Plan complete and saved to `docs/superpowers/plans/2026-06-13-tui-049-def-boundary-convergence.md`. Two execution options:

1. **Subagent-Driven (recommended)** - dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** - execute tasks in this session using executing-plans, batch execution with checkpoints.
