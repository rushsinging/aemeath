# TUI M2 Conversation ToolCall Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入 `ConversationModel → Chat → ChatTurn → ToolCall`，让 `ToolCall.status` 成为 tool 标题图标和颜色的唯一来源。

**Architecture:** 在 M1 的 ViewModel/ViewAssembler 边界上新增 Conversation Model。先接入 tool lifecycle 事件，保留现有 OutputArea legacy path 作兼容，逐步让 OutputViewAssembler 从 ConversationModel 生成 tool blocks，并移除“最后 running header” fallback 与 render 阶段 icon 覆盖。

**Tech Stack:** Rust 2021、现有 `apps/cli` crate、sdk `UiEvent`/`ChatEvent` 链路、ratatui render legacy adapter、`cargo test -p cli`、`.agents/hooks/check-architecture-guards.sh`。

---

## File Structure

- Create: `apps/cli/src/tui/model/mod.rs` — Model Context 出口。
- Create: `apps/cli/src/tui/model/conversation/mod.rs` — Conversation 模块出口。
- Create: `apps/cli/src/tui/model/conversation/ids.rs` — ChatId、ChatTurnId、ToolCallId、ToolStreamKey。
- Create: `apps/cli/src/tui/model/conversation/tool_call.rs` — ToolCall entity 和状态机。
- Create: `apps/cli/src/tui/model/conversation/chat_turn.rs` — ChatTurn entity。
- Create: `apps/cli/src/tui/model/conversation/chat.rs` — Chat entity。
- Create: `apps/cli/src/tui/model/conversation/intent.rs` — ConversationIntent。
- Create: `apps/cli/src/tui/model/conversation/change.rs` — ConversationChange。
- Create: `apps/cli/src/tui/model/conversation/model.rs` — ConversationModel root。
- Modify: `apps/cli/src/tui/mod.rs` — 导出 `model`。
- Modify: `apps/cli/src/tui/view_assembler/output.rs` — 支持从 ConversationModel 生成 tool blocks。
- Modify: `apps/cli/src/tui/core/update/ui_event.rs` — 将 tool 事件同时写入 ConversationModel；迁移完成后删除 output line 直接完成态修改。
- Modify: `apps/cli/src/tui/output_area/render_status.rs` — 不再把完成态 `✓/✗` 覆盖成 `●`。

## Task 1: Add ToolCall state machine

**Files:**
- Create: `apps/cli/src/tui/model/mod.rs`
- Create: `apps/cli/src/tui/model/conversation/mod.rs`
- Create: `apps/cli/src/tui/model/conversation/ids.rs`
- Create: `apps/cli/src/tui/model/conversation/tool_call.rs`
- Modify: `apps/cli/src/tui/mod.rs`

- [ ] **Step 1: Write failing ToolCall tests**

Create `apps/cli/src/tui/model/conversation/tool_call.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};

    fn stream_key() -> ToolStreamKey {
        ToolStreamKey::new(ChatId::new("chat-1"), ChatTurnId::new("turn-1"), "Read", 0)
    }

    #[test]
    fn test_tool_call_binds_id_and_runs() {
        let mut call = ToolCall::pending(stream_key());
        let changes = call.bind(ToolCallId::new("tool-1"), "Read file".to_string());
        assert_eq!(call.id.as_ref().map(AsRef::as_ref), Some("tool-1"));
        assert_eq!(call.status, ToolCallStatus::Running);
        assert_eq!(changes, vec![ToolCallChange::Bound, ToolCallChange::Running]);
    }

    #[test]
    fn test_tool_call_completes_success() {
        let mut call = ToolCall::pending(stream_key());
        call.bind(ToolCallId::new("tool-1"), "Read file".to_string());
        call.complete("ok".to_string(), false);
        assert_eq!(call.status, ToolCallStatus::Success);
        assert_eq!(call.result.as_deref(), Some("ok"));
    }

    #[test]
    fn test_tool_call_completes_error() {
        let mut call = ToolCall::pending(stream_key());
        call.bind(ToolCallId::new("tool-1"), "Read file".to_string());
        call.complete("failed".to_string(), true);
        assert_eq!(call.status, ToolCallStatus::Error);
        assert_eq!(call.result.as_deref(), Some("failed"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::model::conversation::tool_call::tests
```

Expected: FAIL because model modules are missing.

- [ ] **Step 3: Implement ids and ToolCall**

Create `apps/cli/src/tui/model/conversation/ids.rs`:

```rust
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChatId(String);

impl ChatId {
    pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }
}

impl AsRef<str> for ChatId {
    fn as_ref(&self) -> &str { &self.0 }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChatTurnId(String);

impl ChatTurnId {
    pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }
}

impl AsRef<str> for ChatTurnId {
    fn as_ref(&self) -> &str { &self.0 }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ToolCallId(String);

impl ToolCallId {
    pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }
}

impl AsRef<str> for ToolCallId {
    fn as_ref(&self) -> &str { &self.0 }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ToolStreamKey {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
    pub name: String,
    pub index: usize,
}

impl ToolStreamKey {
    pub fn new(chat_id: ChatId, turn_id: ChatTurnId, name: impl Into<String>, index: usize) -> Self {
        Self { chat_id, turn_id, name: name.into(), index }
    }
}
```

Create `apps/cli/src/tui/model/conversation/tool_call.rs`:

```rust
use super::ids::{ToolCallId, ToolStreamKey};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCall {
    pub id: Option<ToolCallId>,
    pub stream_key: ToolStreamKey,
    pub name: String,
    pub args_preview: String,
    pub summary: Option<String>,
    pub status: ToolCallStatus,
    pub result: Option<String>,
    pub activities: Vec<String>,
}

impl ToolCall {
    pub fn pending(stream_key: ToolStreamKey) -> Self {
        Self {
            name: stream_key.name.clone(),
            id: None,
            stream_key,
            args_preview: String::new(),
            summary: None,
            status: ToolCallStatus::PendingArgs,
            result: None,
            activities: Vec::new(),
        }
    }

    pub fn update_args(&mut self, partial_args: impl Into<String>) {
        self.args_preview = partial_args.into();
    }

    pub fn bind(&mut self, id: ToolCallId, summary: String) -> Vec<ToolCallChange> {
        self.id = Some(id);
        self.summary = Some(summary);
        self.status = ToolCallStatus::Running;
        vec![ToolCallChange::Bound, ToolCallChange::Running]
    }

    pub fn complete(&mut self, result: String, is_error: bool) {
        self.result = Some(result);
        self.status = if is_error { ToolCallStatus::Error } else { ToolCallStatus::Success };
    }

    pub fn orphan(&mut self) { self.status = ToolCallStatus::Orphaned; }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolCallStatus {
    PendingArgs,
    Ready,
    Running,
    Success,
    Error,
    Cancelled,
    Orphaned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolCallChange {
    Bound,
    Running,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};

    fn stream_key() -> ToolStreamKey {
        ToolStreamKey::new(ChatId::new("chat-1"), ChatTurnId::new("turn-1"), "Read", 0)
    }

    #[test]
    fn test_tool_call_binds_id_and_runs() {
        let mut call = ToolCall::pending(stream_key());
        let changes = call.bind(ToolCallId::new("tool-1"), "Read file".to_string());
        assert_eq!(call.id.as_ref().map(AsRef::as_ref), Some("tool-1"));
        assert_eq!(call.status, ToolCallStatus::Running);
        assert_eq!(changes, vec![ToolCallChange::Bound, ToolCallChange::Running]);
    }

    #[test]
    fn test_tool_call_completes_success() {
        let mut call = ToolCall::pending(stream_key());
        call.bind(ToolCallId::new("tool-1"), "Read file".to_string());
        call.complete("ok".to_string(), false);
        assert_eq!(call.status, ToolCallStatus::Success);
        assert_eq!(call.result.as_deref(), Some("ok"));
    }

    #[test]
    fn test_tool_call_completes_error() {
        let mut call = ToolCall::pending(stream_key());
        call.bind(ToolCallId::new("tool-1"), "Read file".to_string());
        call.complete("failed".to_string(), true);
        assert_eq!(call.status, ToolCallStatus::Error);
        assert_eq!(call.result.as_deref(), Some("failed"));
    }
}
```

Create `apps/cli/src/tui/model/conversation/mod.rs`:

```rust
pub mod ids;
pub mod tool_call;

pub use ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};
pub use tool_call::{ToolCall, ToolCallStatus};
```

Create `apps/cli/src/tui/model/mod.rs`:

```rust
pub mod conversation;
```

Modify `apps/cli/src/tui/mod.rs`:

```rust
pub mod model;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::model::conversation::tool_call::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/mod.rs apps/cli/src/tui/model
git commit -m "feat: add TUI conversation tool call model"
```

## Task 2: Add ChatTurn and ConversationModel lifecycle

**Files:**
- Create: `apps/cli/src/tui/model/conversation/chat_turn.rs`
- Create: `apps/cli/src/tui/model/conversation/chat.rs`
- Create: `apps/cli/src/tui/model/conversation/intent.rs`
- Create: `apps/cli/src/tui/model/conversation/change.rs`
- Create: `apps/cli/src/tui/model/conversation/model.rs`
- Modify: `apps/cli/src/tui/model/conversation/mod.rs`

- [ ] **Step 1: Write failing ConversationModel tests**

Create `apps/cli/src/tui/model/conversation/model.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::intent::ConversationIntent;

    #[test]
    fn test_conversation_observes_tool_lifecycle() {
        let mut model = ConversationModel::default();
        let changes = model.apply(ConversationIntent::StartChat { submission: "read file".to_string() });
        assert!(changes.iter().any(|change| matches!(change, ConversationChange::ChatStarted { .. })));

        model.apply(ConversationIntent::ObserveToolCallStart { name: "Read".to_string(), index: 0 });
        model.apply(ConversationIntent::ObserveToolCall { id: "tool-1".to_string(), name: "Read".to_string(), index: 0, summary: "Read file".to_string() });
        let changes = model.apply(ConversationIntent::ObserveToolResult { id: "tool-1".to_string(), output: "ok".to_string(), is_error: false });

        assert!(changes.iter().any(|change| matches!(change, ConversationChange::ToolCallCompleted { status, .. } if *status == ToolCallStatus::Success)));
    }

    #[test]
    fn test_conversation_reports_orphan_tool_result() {
        let mut model = ConversationModel::default();
        model.apply(ConversationIntent::StartChat { submission: "read file".to_string() });
        let changes = model.apply(ConversationIntent::ObserveToolResult { id: "missing".to_string(), output: "late".to_string(), is_error: false });
        assert!(changes.iter().any(|change| matches!(change, ConversationChange::OrphanToolResultObserved { id } if id == "missing")));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::model::conversation::model::tests
```

Expected: FAIL because lifecycle files are missing.

- [ ] **Step 3: Implement lifecycle model**

Create `apps/cli/src/tui/model/conversation/change.rs`:

```rust
use super::tool_call::ToolCallStatus;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationChange {
    ChatStarted { chat_id: String },
    ChatTurnStarted { chat_id: String, turn_id: String },
    ToolCallObserved { name: String, index: usize },
    ToolCallBound { id: String, name: String },
    ToolCallCompleted { id: String, status: ToolCallStatus },
    ChatCompleting { chat_id: String },
    ChatCompleted { chat_id: String },
    OrphanToolResultObserved { id: String },
}
```

Create `apps/cli/src/tui/model/conversation/intent.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationIntent {
    StartChat { submission: String },
    ObserveAssistantText { text: String },
    ObserveToolCallStart { name: String, index: usize },
    ObserveToolArguments { name: String, index: usize, partial_args: String },
    ObserveToolCall { id: String, name: String, index: usize, summary: String },
    ObserveToolResult { id: String, output: String, is_error: bool },
    CompleteChat,
}
```

Create `apps/cli/src/tui/model/conversation/chat_turn.rs`:

```rust
use super::ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};
use super::tool_call::{ToolCall, ToolCallStatus};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatTurn {
    pub id: ChatTurnId,
    pub sequence: usize,
    pub status: ChatTurnStatus,
    pub assistant_stream: String,
    pub tool_calls: Vec<ToolCall>,
}

impl ChatTurn {
    pub fn new(id: ChatTurnId, sequence: usize) -> Self {
        Self { id, sequence, status: ChatTurnStatus::Streaming, assistant_stream: String::new(), tool_calls: Vec::new() }
    }

    pub fn observe_tool_start(&mut self, chat_id: ChatId, name: String, index: usize) {
        let key = ToolStreamKey::new(chat_id, self.id.clone(), name, index);
        self.tool_calls.push(ToolCall::pending(key));
        self.status = ChatTurnStatus::ToolCalling;
    }

    pub fn bind_tool(&mut self, id: ToolCallId, name: &str, index: usize, summary: String) -> bool {
        if let Some(call) = self.tool_calls.iter_mut().find(|call| call.stream_key.name == name && call.stream_key.index == index) {
            call.bind(id, summary);
            self.status = ChatTurnStatus::ToolExecuting;
            return true;
        }
        false
    }

    pub fn complete_tool(&mut self, id: &str, output: String, is_error: bool) -> Option<ToolCallStatus> {
        let call = self.tool_calls.iter_mut().find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id))?;
        call.complete(output, is_error);
        let status = call.status;
        if self.tool_calls.iter().all(|call| matches!(call.status, ToolCallStatus::Success | ToolCallStatus::Error | ToolCallStatus::Cancelled | ToolCallStatus::Orphaned)) {
            self.status = ChatTurnStatus::Completing;
        }
        Some(status)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatTurnStatus {
    Streaming,
    ToolCalling,
    ToolExecuting,
    Completing,
    Completed,
    Failed,
}
```

Create `apps/cli/src/tui/model/conversation/chat.rs`:

```rust
use super::chat_turn::ChatTurn;
use super::ids::{ChatId, ChatTurnId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chat {
    pub id: ChatId,
    pub user_submission: String,
    pub status: ChatStatus,
    pub turns: Vec<ChatTurn>,
}

impl Chat {
    pub fn new(id: ChatId, user_submission: String) -> Self {
        Self { id, user_submission, status: ChatStatus::Running, turns: vec![ChatTurn::new(ChatTurnId::new("turn-1"), 0)] }
    }

    pub fn active_turn_mut(&mut self) -> Option<&mut ChatTurn> { self.turns.last_mut() }
    pub fn active_turn(&self) -> Option<&ChatTurn> { self.turns.last() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatStatus {
    Created,
    Running,
    Completing,
    Completed,
    Failed,
    Cancelled,
}
```

Create `apps/cli/src/tui/model/conversation/model.rs`:

```rust
use super::change::ConversationChange;
use super::chat::{Chat, ChatStatus};
use super::ids::{ChatId, ToolCallId};
use super::intent::ConversationIntent;
use super::tool_call::ToolCallStatus;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConversationModel {
    pub chats: Vec<Chat>,
    pub active_chat_id: Option<ChatId>,
    next_chat_sequence: usize,
}

impl ConversationModel {
    pub fn apply(&mut self, intent: ConversationIntent) -> Vec<ConversationChange> {
        match intent {
            ConversationIntent::StartChat { submission } => self.start_chat(submission),
            ConversationIntent::ObserveToolCallStart { name, index } => self.observe_tool_call_start(name, index),
            ConversationIntent::ObserveToolCall { id, name, index, summary } => self.observe_tool_call(id, name, index, summary),
            ConversationIntent::ObserveToolResult { id, output, is_error } => self.observe_tool_result(id, output, is_error),
            ConversationIntent::CompleteChat => self.complete_chat(),
            ConversationIntent::ObserveAssistantText { text } => {
                if let Some(chat) = self.active_chat_mut() {
                    if let Some(turn) = chat.active_turn_mut() { turn.assistant_stream.push_str(&text); }
                }
                Vec::new()
            }
            ConversationIntent::ObserveToolArguments { name, index, partial_args } => {
                if let Some(chat) = self.active_chat_mut() {
                    if let Some(turn) = chat.active_turn_mut() {
                        if let Some(call) = turn.tool_calls.iter_mut().find(|call| call.stream_key.name == name && call.stream_key.index == index) {
                            call.update_args(partial_args);
                        }
                    }
                }
                Vec::new()
            }
        }
    }

    fn start_chat(&mut self, submission: String) -> Vec<ConversationChange> {
        self.next_chat_sequence += 1;
        let chat_id = ChatId::new(format!("chat-{}", self.next_chat_sequence));
        let chat = Chat::new(chat_id.clone(), submission);
        self.active_chat_id = Some(chat_id.clone());
        self.chats.push(chat);
        vec![ConversationChange::ChatStarted { chat_id: chat_id.as_ref().to_string() }, ConversationChange::ChatTurnStarted { chat_id: chat_id.as_ref().to_string(), turn_id: "turn-1".to_string() }]
    }

    fn observe_tool_call_start(&mut self, name: String, index: usize) -> Vec<ConversationChange> {
        let Some(chat_id) = self.active_chat_id.clone() else { return Vec::new(); };
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() { turn.observe_tool_start(chat_id, name.clone(), index); }
        }
        vec![ConversationChange::ToolCallObserved { name, index }]
    }

    fn observe_tool_call(&mut self, id: String, name: String, index: usize, summary: String) -> Vec<ConversationChange> {
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                if turn.bind_tool(ToolCallId::new(id.clone()), &name, index, summary) {
                    return vec![ConversationChange::ToolCallBound { id, name }];
                }
            }
        }
        Vec::new()
    }

    fn observe_tool_result(&mut self, id: String, output: String, is_error: bool) -> Vec<ConversationChange> {
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                if let Some(status) = turn.complete_tool(&id, output, is_error) {
                    return vec![ConversationChange::ToolCallCompleted { id, status }];
                }
            }
        }
        vec![ConversationChange::OrphanToolResultObserved { id }]
    }

    fn complete_chat(&mut self) -> Vec<ConversationChange> {
        if let Some(chat) = self.active_chat_mut() {
            chat.status = ChatStatus::Completing;
            let chat_id = chat.id.as_ref().to_string();
            return vec![ConversationChange::ChatCompleting { chat_id }];
        }
        Vec::new()
    }

    fn active_chat_mut(&mut self) -> Option<&mut Chat> {
        let active = self.active_chat_id.clone()?;
        self.chats.iter_mut().find(|chat| chat.id == active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::intent::ConversationIntent;

    #[test]
    fn test_conversation_observes_tool_lifecycle() {
        let mut model = ConversationModel::default();
        let changes = model.apply(ConversationIntent::StartChat { submission: "read file".to_string() });
        assert!(changes.iter().any(|change| matches!(change, ConversationChange::ChatStarted { .. })));

        model.apply(ConversationIntent::ObserveToolCallStart { name: "Read".to_string(), index: 0 });
        model.apply(ConversationIntent::ObserveToolCall { id: "tool-1".to_string(), name: "Read".to_string(), index: 0, summary: "Read file".to_string() });
        let changes = model.apply(ConversationIntent::ObserveToolResult { id: "tool-1".to_string(), output: "ok".to_string(), is_error: false });

        assert!(changes.iter().any(|change| matches!(change, ConversationChange::ToolCallCompleted { status, .. } if *status == ToolCallStatus::Success)));
    }

    #[test]
    fn test_conversation_reports_orphan_tool_result() {
        let mut model = ConversationModel::default();
        model.apply(ConversationIntent::StartChat { submission: "read file".to_string() });
        let changes = model.apply(ConversationIntent::ObserveToolResult { id: "missing".to_string(), output: "late".to_string(), is_error: false });
        assert!(changes.iter().any(|change| matches!(change, ConversationChange::OrphanToolResultObserved { id } if id == "missing")));
    }
}
```

Update `apps/cli/src/tui/model/conversation/mod.rs`:

```rust
pub mod change;
pub mod chat;
pub mod chat_turn;
pub mod ids;
pub mod intent;
pub mod model;
pub mod tool_call;

pub use change::ConversationChange;
pub use chat::{Chat, ChatStatus};
pub use chat_turn::{ChatTurn, ChatTurnStatus};
pub use ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};
pub use intent::ConversationIntent;
pub use model::ConversationModel;
pub use tool_call::{ToolCall, ToolCallStatus};
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::model::conversation::model::tests tui::model::conversation::tool_call::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/model/conversation
git commit -m "feat: add TUI conversation lifecycle model"
```

## Task 3: Assemble tool blocks from ConversationModel

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output.rs`

- [ ] **Step 1: Add failing assembler test**

Append to `apps/cli/src/tui/view_assembler/output.rs` tests:

```rust
#[test]
fn test_output_assembler_maps_tool_status_to_icon() {
    use crate::tui::model::conversation::{ConversationIntent, ConversationModel};
    use crate::tui::view_model::{OutputBlockView, ToolSemanticStatus};

    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat { submission: "read".to_string() });
    conversation.apply(ConversationIntent::ObserveToolCallStart { name: "Read".to_string(), index: 0 });
    conversation.apply(ConversationIntent::ObserveToolCall { id: "tool-1".to_string(), name: "Read".to_string(), index: 0, summary: "Read file".to_string() });
    conversation.apply(ConversationIntent::ObserveToolResult { id: "tool-1".to_string(), output: "ok".to_string(), is_error: false });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool = vm.blocks.iter().find_map(|block| match block {
        OutputBlockView::ToolCall(tool) => Some(tool),
        _ => None,
    }).expect("tool block");

    assert_eq!(tool.icon, "✓");
    assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::view_assembler::output::tests::test_output_assembler_maps_tool_status_to_icon
```

Expected: FAIL because `assemble_from_conversation` is missing.

- [ ] **Step 3: Implement status mapping**

Add to `apps/cli/src/tui/view_assembler/output.rs`:

```rust
use crate::tui::model::conversation::{ConversationModel, ToolCallStatus};
use crate::tui::view_model::{ToolCallBlockView, ToolSemanticStatus};

impl OutputViewAssembler {
    pub fn assemble_from_conversation(conversation: &ConversationModel, version: u64) -> OutputViewModel {
        let mut blocks = Vec::new();
        for chat in &conversation.chats {
            for turn in &chat.turns {
                for call in &turn.tool_calls {
                    let (icon, semantic_status, style) = map_tool_status(call.status);
                    blocks.push(OutputBlockView::ToolCall(ToolCallBlockView {
                        key: format!("{}/{}/{}", chat.id.as_ref(), turn.id.as_ref(), call.id.as_ref().map(AsRef::as_ref).unwrap_or(&call.name)),
                        chat_id: Some(chat.id.as_ref().to_string()),
                        turn_id: Some(turn.id.as_ref().to_string()),
                        tool_call_id: call.id.as_ref().map(|id| id.as_ref().to_string()),
                        title: call.name.clone(),
                        icon: icon.to_string(),
                        semantic_status,
                        style,
                        args_preview: if call.args_preview.is_empty() { None } else { Some(call.args_preview.clone()) },
                        summary: call.summary.clone(),
                        activity_summary: None,
                        result_summary: call.result.clone(),
                        collapsible: true,
                        collapsed: false,
                    }));
                }
            }
        }
        OutputViewModel { blocks, version, follow_tail_hint: true }
    }
}

fn map_tool_status(status: ToolCallStatus) -> (&'static str, ToolSemanticStatus, SemanticStyle) {
    match status {
        ToolCallStatus::PendingArgs | ToolCallStatus::Ready | ToolCallStatus::Running => ("●", ToolSemanticStatus::Running, SemanticStyle::Running),
        ToolCallStatus::Success => ("✓", ToolSemanticStatus::Success, SemanticStyle::Success),
        ToolCallStatus::Error => ("✗", ToolSemanticStatus::Error, SemanticStyle::Error),
        ToolCallStatus::Cancelled => ("–", ToolSemanticStatus::Cancelled, SemanticStyle::Muted),
        ToolCallStatus::Orphaned => ("?", ToolSemanticStatus::Orphaned, SemanticStyle::Warning),
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::view_assembler::output::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/view_assembler/output.rs
git commit -m "feat: assemble tool blocks from conversation model"
```

## Task 4: Stop render from overwriting completed tool icons

**Files:**
- Modify: `apps/cli/src/tui/output_area/render_status.rs`

- [ ] **Step 1: Add failing regression test**

Append test to `apps/cli/src/tui/output_area/render_status.rs` tests:

```rust
#[test]
fn test_color_tool_call_dots_preserves_success_icon() {
    use crate::tui::output_area::{LineStyle, OutputLine};

    let mut output = OutputArea::new();
    output.lines.push_back(OutputLine {
        content: "✓ Read".to_string(),
        style: LineStyle::ToolCallSuccess,
        ..Default::default()
    });
    output.screen_line_map.push((0, CharIdx::ZERO, CharIdx::new(6)));
    let area = Rect::new(0, 0, 20, 1);
    let mut buf = Buffer::empty(area);
    buf.cell_mut((0, 0)).unwrap().set_char('✓');

    output.color_tool_call_dots(area, &mut buf, 0, 1);

    assert_eq!(buf.cell((0, 0)).unwrap().symbol(), "✓");
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::output_area::render_status::tests::test_color_tool_call_dots_preserves_success_icon
```

Expected before fix: FAIL because current implementation writes `●` for all matched tool status lines.

- [ ] **Step 3: Fix icon overwrite**

In `apps/cli/src/tui/output_area/render_status.rs`, replace:

```rust
cell.set_char('●');
```

with:

```rust
if matches!(line.style, LineStyle::ToolCallRunning) {
    cell.set_char('●');
}
```

- [ ] **Step 4: Run regression test**

```bash
cargo test -p cli tui::output_area::render_status::tests::test_color_tool_call_dots_preserves_success_icon
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/output_area/render_status.rs
git commit -m "fix: preserve completed tool icons during render"
```

## Task 5: Add architecture guard for legacy output fallback

**Files:**
- Modify: `.agents/hooks/check-tui-tea-purity.sh` or create a dedicated guard and call it from `.agents/hooks/check-architecture-guards.sh`

- [ ] **Step 1: Add guard script body**

Create `.agents/hooks/check-tui-output-legacy-guards.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT="${AEMEATH_PROJECT_DIR:-$(pwd)}"
cd "$ROOT"

fail=0

if grep -R "find_last_running\|last running\|最后一个 running\|mark_tool_header_done" apps/cli/src/tui -n --include='*.rs'; then
  echo "[architecture] output legacy fallback is forbidden after TUI M2" >&2
  fail=1
fi

if grep -R "cell\.set_char('●')" apps/cli/src/tui/output_area apps/cli/src/tui/render -n --include='*.rs'; then
  echo "[architecture] render must not overwrite completed tool status icons" >&2
  fail=1
fi

exit "$fail"
```

- [ ] **Step 2: Make guard executable and wire it**

```bash
chmod +x .agents/hooks/check-tui-output-legacy-guards.sh
```

Add this line to `.agents/hooks/check-architecture-guards.sh` after existing TUI guard:

```bash
run_guard "check-tui-output-legacy-guards.sh"
```

Use the helper name already present in `check-architecture-guards.sh`; if the helper is named differently, add the new script consistently with neighboring guard calls.

- [ ] **Step 3: Run guard**

```bash
.agents/hooks/check-tui-output-legacy-guards.sh
.agents/hooks/check-architecture-guards.sh
```

Expected: PASS.

- [ ] **Step 4: Run full verification**

```bash
cargo test -p cli
cargo check -p cli
.agents/hooks/check-architecture-guards.sh
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add .agents/hooks/check-tui-output-legacy-guards.sh .agents/hooks/check-architecture-guards.sh
git commit -m "chore: guard TUI tool status architecture"
```

## Final verification

Run:

```bash
cargo test -p cli
cargo check -p cli
.agents/hooks/check-architecture-guards.sh
```

Expected: all PASS.

M2 is complete when a tool result updates `ConversationModel.ToolCall.status`, OutputViewAssembler maps that status to `✓/✗/●`, and render no longer overwrites completed icons.
