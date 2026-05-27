# TUI M3 Input Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入 `InputModel`，让输入编辑、提交、排队和 prompt answer 语义通过 `InputIntent → InputChange` 表达。

**Architecture:** 保留现有 `InputArea` 作为 UI 组件与 legacy adapter，新增纯 `model/input` 管理 buffer、cursor、selection、history、completion 和 attachments。Key handling 逐步变成 Intent，submit 只产出不可变 `InputSubmission`，是否启动 chat/queue/answer prompt 由 coordinator 决定。

**Tech Stack:** Rust 2021、tui-textarea legacy UI、现有 `apps/cli` crate、`cargo test -p cli`、TUI TEA purity guard。

---

## File Structure

- Create: `apps/cli/src/tui/model/input/mod.rs` — InputModel 模块出口。
- Create: `apps/cli/src/tui/model/input/document.rs` — InputDocument buffer/cursor/selection。
- Create: `apps/cli/src/tui/model/input/submission.rs` — InputSubmission。
- Create: `apps/cli/src/tui/model/input/intent.rs` — InputIntent。
- Create: `apps/cli/src/tui/model/input/change.rs` — InputChange。
- Create: `apps/cli/src/tui/model/input/model.rs` — InputModel root。
- Create: `apps/cli/src/tui/model/input/history.rs` — history state。
- Create: `apps/cli/src/tui/model/input/completion.rs` — completion state。
- Create: `apps/cli/src/tui/model/input/attachment.rs` — attachment value object。
- Modify: `apps/cli/src/tui/model/mod.rs` — 导出 input。
- Modify: `apps/cli/src/tui/view_assembler/input.rs` — 从 InputModel 组装 InputAreaViewModel。
- Later Modify: `apps/cli/src/tui/core/update/key.rs` and `enter.rs` — 将 key/submit 逐步路由为 InputIntent。

## Task 1: Add InputDocument

**Files:**
- Create: `apps/cli/src/tui/model/input/document.rs`
- Create: `apps/cli/src/tui/model/input/mod.rs`
- Modify: `apps/cli/src/tui/model/mod.rs`

- [ ] **Step 1: Write failing document tests**

Create `apps/cli/src/tui/model/input/document.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_text_advances_cursor() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc");
        assert_eq!(doc.buffer, "abc");
        assert_eq!(doc.cursor, 3);
    }

    #[test]
    fn test_move_cursor_clamps_to_buffer() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc");
        doc.move_cursor(99);
        assert_eq!(doc.cursor, 3);
        doc.move_cursor(0);
        assert_eq!(doc.cursor, 0);
    }

    #[test]
    fn test_delete_backward_at_start_is_noop() {
        let mut doc = InputDocument::default();
        doc.delete_backward();
        assert_eq!(doc.buffer, "");
        assert_eq!(doc.cursor, 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::model::input::document::tests
```

Expected: FAIL because input model does not exist.

- [ ] **Step 3: Implement InputDocument**

Create `apps/cli/src/tui/model/input/document.rs`:

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputDocument {
    pub buffer: String,
    pub cursor: usize,
    pub selection: Option<InputSelection>,
}

impl InputDocument {
    pub fn insert_text(&mut self, text: &str) {
        let cursor = self.cursor.min(self.buffer.len());
        self.buffer.insert_str(cursor, text);
        self.cursor = cursor + text.len();
        self.selection = None;
    }

    pub fn move_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.buffer.len());
        self.selection = None;
    }

    pub fn delete_backward(&mut self) {
        if self.cursor == 0 { return; }
        let remove_at = self.cursor - 1;
        self.buffer.remove(remove_at);
        self.cursor = remove_at;
        self.selection = None;
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.selection = None;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputSelection {
    pub start: usize,
    pub end: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_text_advances_cursor() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc");
        assert_eq!(doc.buffer, "abc");
        assert_eq!(doc.cursor, 3);
    }

    #[test]
    fn test_move_cursor_clamps_to_buffer() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc");
        doc.move_cursor(99);
        assert_eq!(doc.cursor, 3);
        doc.move_cursor(0);
        assert_eq!(doc.cursor, 0);
    }

    #[test]
    fn test_delete_backward_at_start_is_noop() {
        let mut doc = InputDocument::default();
        doc.delete_backward();
        assert_eq!(doc.buffer, "");
        assert_eq!(doc.cursor, 0);
    }
}
```

Create `apps/cli/src/tui/model/input/mod.rs`:

```rust
pub mod document;

pub use document::{InputDocument, InputSelection};
```

Modify `apps/cli/src/tui/model/mod.rs`:

```rust
pub mod conversation;
pub mod input;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::model::input::document::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/model
git commit -m "feat: add TUI input document model"
```

## Task 2: Add InputIntent, InputChange, InputSubmission and InputModel

**Files:**
- Create: `apps/cli/src/tui/model/input/submission.rs`
- Create: `apps/cli/src/tui/model/input/intent.rs`
- Create: `apps/cli/src/tui/model/input/change.rs`
- Create: `apps/cli/src/tui/model/input/model.rs`
- Modify: `apps/cli/src/tui/model/input/mod.rs`

- [ ] **Step 1: Write failing InputModel tests**

Create `apps/cli/src/tui/model/input/model.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::input::intent::InputIntent;

    #[test]
    fn test_submit_emits_submission_and_clears_buffer() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("hello".to_string()));
        let changes = model.apply(InputIntent::Submit);
        assert!(changes.iter().any(|change| matches!(change, InputChange::Submitted { submission } if submission.text == "hello")));
        assert_eq!(model.document.buffer, "");
    }

    #[test]
    fn test_empty_submit_is_ignored() {
        let mut model = InputModel::default();
        let changes = model.apply(InputIntent::Submit);
        assert!(changes.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::model::input::model::tests
```

Expected: FAIL because model files are missing.

- [ ] **Step 3: Implement InputModel**

Create `apps/cli/src/tui/model/input/submission.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSubmission {
    pub text: String,
    pub attachments: Vec<InputAttachment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputAttachment {
    pub label: String,
    pub path: Option<String>,
}
```

Create `apps/cli/src/tui/model/input/intent.rs`:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputIntent {
    InsertText(String),
    MoveCursor(usize),
    DeleteBackward,
    Submit,
    Clear,
}
```

Create `apps/cli/src/tui/model/input/change.rs`:

```rust
use super::submission::InputSubmission;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputChange {
    TextChanged { text: String, cursor: usize },
    CursorMoved { cursor: usize },
    Submitted { submission: InputSubmission },
    Cleared,
}
```

Create `apps/cli/src/tui/model/input/model.rs`:

```rust
use super::change::InputChange;
use super::document::InputDocument;
use super::intent::InputIntent;
use super::submission::InputSubmission;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputModel {
    pub document: InputDocument,
    pub mode: InputMode,
}

impl InputModel {
    pub fn apply(&mut self, intent: InputIntent) -> Vec<InputChange> {
        match intent {
            InputIntent::InsertText(text) => {
                self.document.insert_text(&text);
                vec![InputChange::TextChanged { text: self.document.buffer.clone(), cursor: self.document.cursor }]
            }
            InputIntent::MoveCursor(cursor) => {
                self.document.move_cursor(cursor);
                vec![InputChange::CursorMoved { cursor: self.document.cursor }]
            }
            InputIntent::DeleteBackward => {
                self.document.delete_backward();
                vec![InputChange::TextChanged { text: self.document.buffer.clone(), cursor: self.document.cursor }]
            }
            InputIntent::Submit => {
                let text = self.document.buffer.trim().to_string();
                if text.is_empty() { return Vec::new(); }
                let submission = InputSubmission { text, attachments: Vec::new() };
                self.document.clear();
                vec![InputChange::Submitted { submission }]
            }
            InputIntent::Clear => {
                self.document.clear();
                vec![InputChange::Cleared]
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InputMode {
    #[default]
    Editing,
    Selecting,
    Completion,
    PromptAnswer,
    Disabled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::input::intent::InputIntent;

    #[test]
    fn test_submit_emits_submission_and_clears_buffer() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("hello".to_string()));
        let changes = model.apply(InputIntent::Submit);
        assert!(changes.iter().any(|change| matches!(change, InputChange::Submitted { submission } if submission.text == "hello")));
        assert_eq!(model.document.buffer, "");
    }

    #[test]
    fn test_empty_submit_is_ignored() {
        let mut model = InputModel::default();
        let changes = model.apply(InputIntent::Submit);
        assert!(changes.is_empty());
    }
}
```

Update `apps/cli/src/tui/model/input/mod.rs`:

```rust
pub mod change;
pub mod document;
pub mod intent;
pub mod model;
pub mod submission;

pub use change::InputChange;
pub use document::{InputDocument, InputSelection};
pub use intent::InputIntent;
pub use model::{InputMode, InputModel};
pub use submission::{InputAttachment, InputSubmission};
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p cli tui::model::input::model::tests tui::model::input::document::tests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/model/input
git commit -m "feat: add TUI input model intents"
```

## Task 3: Assemble InputAreaViewModel from InputModel

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/input.rs`

- [ ] **Step 1: Add failing assembler test**

Append to `apps/cli/src/tui/view_assembler/input.rs`:

```rust
#[cfg(test)]
mod tests {
    use crate::tui::model::input::{InputIntent, InputModel};

    use super::InputViewAssembler;

    #[test]
    fn test_input_assembler_reads_input_model() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("hello".to_string()));
        let vm = InputViewAssembler::assemble_from_model(&model, None);
        assert_eq!(vm.text, "hello");
        assert_eq!(vm.cursor, 5);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::view_assembler::input::tests::test_input_assembler_reads_input_model
```

Expected: FAIL because `assemble_from_model` is missing.

- [ ] **Step 3: Implement model assembler**

Modify `apps/cli/src/tui/view_assembler/input.rs`:

```rust
use crate::tui::model::input::{InputMode, InputModel};
use crate::tui::view_model::InputAreaViewModel;

pub struct InputViewAssembler;

impl InputViewAssembler {
    pub fn assemble_text(text: &str, cursor: usize) -> InputAreaViewModel {
        InputAreaViewModel { text: text.to_string(), cursor, placeholder: None, mode_label: None, queued_hint: None, disabled_reason: None }
    }

    pub fn assemble_from_model(model: &InputModel, queued_hint: Option<String>) -> InputAreaViewModel {
        InputAreaViewModel {
            text: model.document.buffer.clone(),
            cursor: model.document.cursor,
            placeholder: if model.document.buffer.is_empty() { Some("输入消息，Enter 发送".to_string()) } else { None },
            mode_label: Some(match model.mode {
                InputMode::Editing => "EDIT".to_string(),
                InputMode::Selecting => "SELECT".to_string(),
                InputMode::Completion => "COMPLETE".to_string(),
                InputMode::PromptAnswer => "PROMPT".to_string(),
                InputMode::Disabled => "DISABLED".to_string(),
            }),
            queued_hint,
            disabled_reason: if model.mode == InputMode::Disabled { Some("输入暂不可用".to_string()) } else { None },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::model::input::{InputIntent, InputModel};

    use super::InputViewAssembler;

    #[test]
    fn test_input_assembler_reads_input_model() {
        let mut model = InputModel::default();
        model.apply(InputIntent::InsertText("hello".to_string()));
        let vm = InputViewAssembler::assemble_from_model(&model, None);
        assert_eq!(vm.text, "hello");
        assert_eq!(vm.cursor, 5);
    }
}
```

- [ ] **Step 4: Run test**

```bash
cargo test -p cli tui::view_assembler::input::tests::test_input_assembler_reads_input_model
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/view_assembler/input.rs
git commit -m "feat: assemble input view model from input model"
```

## Task 4: Add coordinator seam for submit decisions

**Files:**
- Create: `apps/cli/src/tui/update/mod.rs`
- Create: `apps/cli/src/tui/update/input_mapper.rs`
- Modify: `apps/cli/src/tui/mod.rs`

- [ ] **Step 1: Write failing routing test**

Create `apps/cli/src/tui/update/input_mapper.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::input::InputSubmission;

    #[test]
    fn test_route_submission_starts_chat_when_idle() {
        let route = route_submission(InputSubmission { text: "hello".to_string(), attachments: Vec::new() }, ConversationAvailability::Idle, false);
        assert!(matches!(route, SubmissionRoute::StartChat { .. }));
    }

    #[test]
    fn test_route_submission_queues_when_running() {
        let route = route_submission(InputSubmission { text: "hello".to_string(), attachments: Vec::new() }, ConversationAvailability::Running, false);
        assert!(matches!(route, SubmissionRoute::QueueSubmission { .. }));
    }

    #[test]
    fn test_route_submission_answers_prompt_first() {
        let route = route_submission(InputSubmission { text: "yes".to_string(), attachments: Vec::new() }, ConversationAvailability::Idle, true);
        assert!(matches!(route, SubmissionRoute::AnswerPrompt { .. }));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::update::input_mapper::tests
```

Expected: FAIL because update module is missing.

- [ ] **Step 3: Implement routing seam**

Create `apps/cli/src/tui/update/input_mapper.rs`:

```rust
use crate::tui::model::input::InputSubmission;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationAvailability {
    Idle,
    Running,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmissionRoute {
    StartChat { submission: InputSubmission },
    QueueSubmission { submission: InputSubmission },
    AnswerPrompt { text: String },
}

pub fn route_submission(submission: InputSubmission, conversation: ConversationAvailability, prompt_active: bool) -> SubmissionRoute {
    if prompt_active {
        return SubmissionRoute::AnswerPrompt { text: submission.text };
    }
    match conversation {
        ConversationAvailability::Idle => SubmissionRoute::StartChat { submission },
        ConversationAvailability::Running => SubmissionRoute::QueueSubmission { submission },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::input::InputSubmission;

    #[test]
    fn test_route_submission_starts_chat_when_idle() {
        let route = route_submission(InputSubmission { text: "hello".to_string(), attachments: Vec::new() }, ConversationAvailability::Idle, false);
        assert!(matches!(route, SubmissionRoute::StartChat { .. }));
    }

    #[test]
    fn test_route_submission_queues_when_running() {
        let route = route_submission(InputSubmission { text: "hello".to_string(), attachments: Vec::new() }, ConversationAvailability::Running, false);
        assert!(matches!(route, SubmissionRoute::QueueSubmission { .. }));
    }

    #[test]
    fn test_route_submission_answers_prompt_first() {
        let route = route_submission(InputSubmission { text: "yes".to_string(), attachments: Vec::new() }, ConversationAvailability::Idle, true);
        assert!(matches!(route, SubmissionRoute::AnswerPrompt { .. }));
    }
}
```

Create `apps/cli/src/tui/update/mod.rs`:

```rust
pub mod input_mapper;
```

Modify `apps/cli/src/tui/mod.rs`:

```rust
pub mod update;
```

- [ ] **Step 4: Run tests and check**

```bash
cargo test -p cli tui::update::input_mapper::tests
cargo check -p cli
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/mod.rs apps/cli/src/tui/update
git commit -m "feat: add TUI input submission routing seam"
```

## Final verification

Run:

```bash
cargo test -p cli tui::model::input tui::view_assembler::input tui::update::input_mapper
cargo check -p cli
.agents/hooks/check-architecture-guards.sh
```

Expected: all PASS.

M3 is complete when input editing state can be represented by `InputModel`, submit emits `InputSubmission`, and routing decisions have a coordinator seam instead of being embedded in input rendering.
