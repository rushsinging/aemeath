# Image Placeholder In Input 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
> **对应 Issue:** rushsinging/aemeath#279

**Goal:** 将粘贴的图片以 `[Image #N]` 占位符纳入 `InputDocument` 单一真相，效仿 `CopiedTextSpan` 的 span 偏移机制，使图片在输入内容区可见、可删除、与文本混合排列。

**Architecture:** `InputDocument` 新增 `image_spans: Vec<ImageSpan>`。图片占位文本插入 buffer 并记录 span 区间；复用与 `copied_text_spans` 相同的偏移维护逻辑（DRY）。提交时 `submit_text()` 展开 copied text 原文、剔除 image 占位；`drain_images()` 按序返回图片数据。移除 `ChatState.pending_images` 双真相及全部 `SetAttachmentCount` / `AttachmentChanged` / `InputAttachment` 空壳。

**Tech Stack:** Rust, ratatui, tui-textarea; Elm 架构（intent → model → change → effect）。

**编号策略：** 固定不重排——`[Image #N]` 的 `N` = 插入时已有 image span 数 + 1，删除后编号留空洞（如 `#1 #3`）。

**执行策略说明：** 本重构高度耦合（删除 `InputSubmission.attachments` 字段会影响所有构造点），各文件改动无法独立编译。因此按文件拆分为可审阅的修改单元，执行阶段一次性完成后统一编译验证。Phase 1-2 是增量（新增类型/方法），Phase 3-6 是迁移+清理，Phase 3 开始删除旧字段后须连续完成至 Phase 7 编译通过。

---

## File Structure

| 文件 | 操作 | 职责 |
|------|------|------|
| `apps/cli/src/tui/model/input/image_span.rs` | **新建** | `ImageSpan` 结构体 |
| `apps/cli/src/tui/model/input/document.rs` | 修改 | 新增 `image_spans`、`insert_image`、`submit_text`、`drain_images`；抽象 span 偏移 |
| `apps/cli/src/tui/model/input/document_tests.rs` | 修改 | 新增图片 span 测试 |
| `apps/cli/src/tui/model/input.rs` | 修改 | 注册 `image_span` 模块；删除 `attachment` 模块 |
| `apps/cli/src/tui/model/input/submission.rs` | 修改 | `attachments` → `images: Vec<sdk::ClipboardImageView>` |
| `apps/cli/src/tui/model/input/intent.rs` | 修改 | 新增 `InsertImage`；删除 `SetAttachmentCount` |
| `apps/cli/src/tui/model/input/model.rs` | 修改 | 处理 `InsertImage`；submit 取图片；删除 `attachments` |
| `apps/cli/src/tui/model/input/change.rs` | 修改 | 删除 `AttachmentChanged` |
| `apps/cli/src/tui/model/input/attachment.rs` | **删除** | 不再使用 |
| `apps/cli/src/tui/app/state/chat.rs` | 修改 | 删除 `pending_images` 字段及 4 个方法 |
| `apps/cli/src/tui/app/state/tests.rs` | 修改 | 删除 pending_images 测试 |
| `apps/cli/src/tui/view_model/input.rs` | 修改 | 删除 `pending_images` 字段 |
| `apps/cli/src/tui/view_assembler/input.rs` | 修改 | 删除 `pending_images` 参数 |
| `apps/cli/src/tui/render/input/input_area/render.rs` | 修改 | 标题固定 ` Input ` |
| `apps/cli/src/tui/app.rs` | 修改 | 删除 `pending_image_count()` 传参 |
| `apps/cli/src/tui/effect/executor.rs` | 修改 | `accept_pending_clipboard_image` → `InsertImage` |
| `apps/cli/src/tui/app/update/ui_event.rs` | 修改 | `ClipboardImage` → `InsertImage` |
| `apps/cli/src/tui/app/update/enter.rs` | 修改 | 从 `submission.images` 取图片 |
| `apps/cli/src/tui/app/slash.rs` | 修改 | `/images`、`/clear-images`、`/clear` 改读 document |
| `apps/cli/src/tui/effect/session/resume.rs` | 修改 | 清 document 代替 `clear_pending_images` |
| `apps/cli/src/tui/update/coordinator.rs` | 修改 | 删除 `AttachmentChanged` 分支 |
| `apps/cli/src/tui/adapter/input.rs` | 修改 | 测试更新 `InputSubmission` 构造 |
| `apps/cli/src/tui/model/input/input_model_tests.rs` | 修改 | 测试更新 |
| `apps/cli/src/tui/model/input/change.rs` (tests) | 修改 | 测试更新 |

---

## Phase 1: 核心数据模型（增量新增）

### Task 1: 新建 ImageSpan 结构

**Files:**
- Create: `apps/cli/src/tui/model/input/image_span.rs`
- Modify: `apps/cli/src/tui/model/input.rs:1` (注册模块)

- [ ] **Step 1: 创建 `image_span.rs`**

```rust
use sdk::ClipboardImageView;

/// 输入文档中图片占位符的区间记录。
///
/// `placeholder`（如 `[Image #1]`）出现在 buffer 的 `[start, end)` 区间，
/// `index` 是插入时分配的序号（固定不重排，删除后留空洞），
/// `image` 持有实际的图片数据。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageSpan {
    pub index: usize,
    pub image: ClipboardImageView,
    pub start: usize,
    pub end: usize,
}

impl ImageSpan {
    pub fn new(index: usize, image: ClipboardImageView, start: usize, end: usize) -> Self {
        Self {
            index,
            image,
            start,
            end,
        }
    }

    /// 该 span 在 buffer 中占用的占位文本
    pub fn placeholder(&self) -> String {
        format!("[Image #{}]", self.index)
    }
}
```

- [ ] **Step 2: 在 `input.rs` 注册模块**

在 `apps/cli/src/tui/model/input.rs` 中，删除 `pub mod attachment;` 并新增 `pub mod image_span;`：

```rust
pub mod change;
pub mod completion;
pub mod completion_item;
pub mod copied_text;
pub mod document;
pub mod history;
#[cfg(test)]
mod input_model_tests;
pub mod image_span;
pub mod intent;
pub mod mode;
pub mod model;
pub mod submission;
```

---

### Task 2: 重构 document.rs — 抽象 span 偏移 + 新增 image_spans

**Files:**
- Modify: `apps/cli/src/tui/model/input/document.rs`

这是核心任务。将 `copied_text_spans` 的偏移维护逻辑抽象为通用 trait/函数，让 `ImageSpan` 共用。

- [ ] **Step 1: 修改 import 与 struct 定义**

文件顶部 import 新增 `ImageSpan`，struct 新增 `image_spans` 字段：

```rust
use super::copied_text::CopiedTextSpan;
use super::image_span::ImageSpan;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputDocument {
    pub buffer: String,
    pub cursor: usize,
    pub copied_text_spans: Vec<CopiedTextSpan>,
    pub image_spans: Vec<ImageSpan>,
}
```

- [ ] **Step 2: 新增 `insert_image` 方法**

在 `insert_pasted_text` 方法之后新增：

```rust
/// 插入图片占位符 `[Image #N]` 并记录 span。序号固定不重排。
pub fn insert_image(&mut self, image: sdk::ClipboardImageView) {
    let index = self.image_spans.len() + 1;
    let placeholder = format!("[Image #{index}]");
    let cursor = clamp_to_char_boundary(&self.buffer, self.cursor.min(self.buffer.len()));
    self.buffer.insert_str(cursor, &placeholder);
    self.shift_spans_for_insert(cursor, placeholder.len());
    let end = cursor + placeholder.len();
    self.image_spans
        .push(ImageSpan::new(index, image, cursor, end));
    self.cursor = end;
}
```

- [ ] **Step 3: 新增 `submit_text` 与 `drain_images` 方法**

```rust
/// 提交文本：展开 copied text 原文、剔除 image 占位。
pub fn submit_text(&self) -> String {
    let mut text = self.expand_copied_text();
    if self.image_spans.is_empty() {
        return text;
    }
    // image_spans 的占位文本是纯 ASCII，按 start 倒序移除，避免偏移重算
    let mut spans = self.image_spans.clone();
    spans.sort_by_key(|span| std::cmp::Reverse(span.start));
    for span in &spans {
        let placeholder = span.placeholder();
        if text.get(span.start..span.end) == Some(placeholder.as_str()) {
            text.replace_range(span.start..span.end, "");
        }
    }
    text.trim().to_string()
}

/// 按文档中出现顺序取出全部图片数据。
pub fn drain_images(&mut self) -> Vec<sdk::ClipboardImageView> {
    let mut spans = std::mem::take(&mut self.image_spans);
    spans.sort_by_key(|span| span.start);
    spans.into_iter().map(|span| span.image).collect()
}
```

- [ ] **Step 4: 修改 `delete_backward` 和 `delete_word_before_cursor` 同时检测 image span**

将 `copied_text_span_for_backward_delete` 泛化为同时检查两类 span。在现有方法中新增 image span 检测：

```rust
pub fn delete_backward(&mut self) {
    if self.cursor == 0 {
        return;
    }
    if let Some((start, end)) = self.atomic_span_for_backward_delete() {
        self.delete_range(start, end);
        return;
    }
    let old_cursor = self.cursor;
    self.move_left();
    self.delete_range(self.cursor, old_cursor);
}

pub fn delete_word_before_cursor(&mut self) {
    if self.cursor == 0 {
        return;
    }
    if let Some((start, end)) = self.atomic_span_for_backward_delete() {
        self.delete_range(start, end);
        return;
    }
    // ...（保持原有 word 删除逻辑不变）
}
```

新增 `atomic_span_for_backward_delete`（替代 `copied_text_span_for_backward_delete`）：

```rust
/// 光标紧邻 span 末尾时，返回该 span 的区间，实现原子删除。
fn atomic_span_for_backward_delete(&self) -> Option<(usize, usize)> {
    self.copied_text_spans
        .iter()
        .find(|span| self.cursor > span.start && self.cursor <= span.end)
        .map(|span| (span.start, span.end))
        .or_else(|| {
            self.image_spans
                .iter()
                .find(|span| self.cursor > span.start && self.cursor <= span.end)
                .map(|span| (span.start, span.end))
        })
}
```

删除旧的 `copied_text_span_for_backward_delete` 方法。

- [ ] **Step 5: 修改 `delete_range` 同步清理 image_spans**

```rust
fn delete_range(&mut self, start: usize, end: usize) {
    let start = clamp_to_char_boundary(&self.buffer, start.min(self.buffer.len()));
    let end = clamp_to_char_boundary(&self.buffer, end.min(self.buffer.len()));
    if start >= end {
        self.cursor = start;
        return;
    }
    self.buffer.drain(start..end);
    let deleted_len = end - start;
    self.copied_text_spans
        .retain(|span| !(span.start >= start && span.end <= end));
    self.image_spans
        .retain(|span| !(span.start >= start && span.end <= end));
    for span in &mut self.copied_text_spans {
        if span.start >= end {
            span.start -= deleted_len;
            span.end -= deleted_len;
        }
    }
    for span in &mut self.image_spans {
        if span.start >= end {
            span.start -= deleted_len;
            span.end -= deleted_len;
        }
    }
    self.cursor = start;
}
```

- [ ] **Step 6: 修改 `shift_spans_for_insert` 同步移动 image_spans**

```rust
fn shift_spans_for_insert(&mut self, start: usize, len: usize) {
    for span in &mut self.copied_text_spans {
        if span.start >= start {
            span.start += len;
            span.end += len;
        }
    }
    for span in &mut self.image_spans {
        if span.start >= start {
            span.start += len;
            span.end += len;
        }
    }
}
```

- [ ] **Step 7: 修改 `clear` 和 `replace_text` 同步清 image_spans**

```rust
pub fn replace_text(&mut self, text: String) {
    self.buffer = text;
    self.cursor = self.buffer.len();
    self.copied_text_spans.clear();
    self.image_spans.clear();
}

pub fn clear(&mut self) {
    self.buffer.clear();
    self.cursor = 0;
    self.copied_text_spans.clear();
    self.image_spans.clear();
}
```

---

### Task 3: document_tests.rs 新增图片 span 测试

**Files:**
- Modify: `apps/cli/src/tui/model/input/document_tests.rs`

- [ ] **Step 1: 新增测试（追加到文件末尾）**

```rust
// === ImageSpan 测试 ===

fn make_test_image(size: usize) -> sdk::ClipboardImageView {
    sdk::ClipboardImageView {
        base64: "img".to_string(),
        media_type: "image/png".to_string(),
        final_size: size,
        display_path: None,
        width: None,
        height: None,
    }
}

#[test]
fn test_insert_image_adds_placeholder_and_span() {
    let mut doc = InputDocument::default();
    doc.insert_text("hello ");
    doc.insert_image(make_test_image(100));
    assert_eq!(doc.buffer, "hello [Image #1]");
    assert_eq!(doc.cursor, "hello [Image #1]".len());
    assert_eq!(doc.image_spans.len(), 1);
    assert_eq!(doc.image_spans[0].index, 1);
}

#[test]
fn test_insert_multiple_images_assigns_sequential_index() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    doc.insert_text(" ");
    doc.insert_image(make_test_image(20));
    assert_eq!(doc.buffer, "[Image #1] [Image #2]");
    assert_eq!(doc.image_spans.len(), 2);
}

#[test]
fn test_delete_backward_atomically_removes_image() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    // cursor 在占位符末尾
    assert_eq!(doc.cursor, "[Image #1]".len());
    doc.delete_backward();
    assert_eq!(doc.buffer, "");
    assert!(doc.image_spans.is_empty());
}

#[test]
fn test_delete_backward_removes_image_in_middle() {
    let mut doc = InputDocument::default();
    doc.insert_text("a");
    doc.insert_image(make_test_image(10));
    doc.insert_text("b");
    // a[Image #1]b, cursor 在末尾 'b' 之后
    // 移到占位符末尾（'b' 之前）
    doc.move_cursor("a[Image #1]".len());
    doc.delete_backward();
    assert_eq!(doc.buffer, "ab");
    assert!(doc.image_spans.is_empty());
}

#[test]
fn test_delete_image_preserves_index_hole() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10)); // #1
    doc.insert_image(make_test_image(20)); // #2
    doc.insert_image(make_test_image(30)); // #3
    // buffer = "[Image #1][Image #2][Image #3]"
    // 删除 #2（中间）
    doc.move_cursor("[Image #1]".len());
    doc.delete_backward(); // 删除 #1... 不对，这里删除的是光标前的 span
    // 重新设计：移动光标到 #2 末尾后 delete_backward
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    doc.insert_image(make_test_image(20));
    doc.insert_image(make_test_image(30));
    // 光标在 #2 末尾（#3 之前）
    let pos = "[Image #1][Image #2]".len();
    doc.move_cursor(pos);
    doc.delete_backward(); // 原子删除 #2
    assert_eq!(doc.buffer, "[Image #1][Image #3]");
    assert_eq!(doc.image_spans.len(), 2);
    // 编号保留原始 index
    assert_eq!(doc.image_spans[0].index, 1);
    assert_eq!(doc.image_spans[1].index, 3);
}

#[test]
fn test_submit_text_strips_image_placeholders() {
    let mut doc = InputDocument::default();
    doc.insert_text("look at ");
    doc.insert_image(make_test_image(10));
    doc.insert_text(" this");
    assert_eq!(doc.submit_text(), "look at  this".trim());
}

#[test]
fn test_drain_images_returns_in_order() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    doc.insert_text(" mid ");
    doc.insert_image(make_test_image(20));
    let images = doc.drain_images();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].final_size, 10);
    assert_eq!(images[1].final_size, 20);
    assert!(doc.image_spans.is_empty());
}

#[test]
fn test_submit_text_expands_copied_text_and_strips_images() {
    let mut doc = InputDocument::default();
    doc.insert_text("see ");
    doc.insert_pasted_text("a\nb\nc\nd"); // 变成 [Copied 4 lines]
    doc.insert_text(" and ");
    doc.insert_image(make_test_image(10));
    let text = doc.submit_text();
    assert!(text.contains("see a\nb\nc\nd and"));
    assert!(!text.contains("[Image #"));
    assert!(!text.contains("[Copied"));
}

#[test]
fn test_clear_removes_image_spans() {
    let mut doc = InputDocument::default();
    doc.insert_image(make_test_image(10));
    doc.clear();
    assert!(doc.image_spans.is_empty());
    assert!(doc.buffer.is_empty());
}
```

---

### Task 4: submission.rs — images 替换 attachments

**Files:**
- Modify: `apps/cli/src/tui/model/input/submission.rs`

- [ ] **Step 1: 替换字段**

```rust
pub struct InputSubmission {
    pub text: String,
    pub display_text: String,
    pub images: Vec<sdk::ClipboardImageView>,
}
```

> 注意：`InputSubmission` 不再 import `InputAttachment`。`display_text` 保留 buffer 原文（含占位），用于历史回显；`text` 是提交给 LLM 的展开文本。

---

### Task 5: intent.rs — 新增 InsertImage，删除 SetAttachmentCount

**Files:**
- Modify: `apps/cli/src/tui/model/input/intent.rs`

- [ ] **Step 1: 替换 intent**

删除 `SetAttachmentCount(usize)`，新增 `InsertImage(sdk::ClipboardImageView)`。同时删除 `SetMode` 依赖无关，保持不变。

```rust
use super::completion_item::CompletionItem;
use super::mode::InputMode;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputIntent {
    InsertChar(char),
    InsertText(String),
    InsertPastedText(String),
    InsertImage(sdk::ClipboardImageView),
    ReplaceText(String),
    MoveCursor(usize),
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorHome,
    MoveCursorEnd,
    InsertNewline,
    DeleteBackward,
    DeleteWordBeforeCursor,
    DeleteForward,
    MoveHistoryPrevious,
    MoveHistoryNext,
    ReplaceHistory(Vec<String>),
    SetCompletions {
        query: String,
        items: Vec<CompletionItem>,
    },
    SelectCompletionNext,
    SelectCompletionPrevious,
    AcceptCompletion,
    AcceptCompletionValue(String),
    SetMode(InputMode),
    Submit,
    Clear,
}
```

---

### Task 6: model.rs — 处理 InsertImage，submit 取图片，移除 attachments

**Files:**
- Modify: `apps/cli/src/tui/model/input/model.rs`

- [ ] **Step 1: 修改 struct 与 import**

删除 `use super::attachment::InputAttachment;`。struct 删除 `attachments` 字段：

```rust
use super::change::InputChange;
use super::completion::InputCompletion;
use super::document::InputDocument;
use super::history::InputHistory;
use super::intent::InputIntent;
use super::mode::InputMode;
use super::submission::InputSubmission;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputModel {
    pub document: InputDocument,
    pub history: InputHistory,
    pub completion: InputCompletion,
    pub mode: InputMode,
}
```

- [ ] **Step 2: 在 `apply` match 中处理新 intent，删除旧 intent**

删除 `InputIntent::SetAttachmentCount(count)` 分支。新增 `InputIntent::InsertImage` 分支：

```rust
InputIntent::InsertImage(image) => {
    self.completion.clear();
    self.history.selected_index = None;
    self.document.insert_image(image);
    self.text_changed()
}
```

- [ ] **Step 3: 修改 `submit()` 方法**

```rust
fn submit(&mut self) -> Vec<InputChange> {
    let text = self.document.submit_text();
    let display_text = self.document.display_text();
    let images = self.document.drain_images();
    let submission = InputSubmission {
        text,
        display_text,
        images,
    };
    self.history.entries.push(submission.display_text.clone());
    self.history.selected_index = None;
    self.history.saved_input.clear();
    self.completion.clear();
    self.mode = InputMode::Normal;
    self.document.clear();
    vec![
        InputChange::Submitted { submission },
        InputChange::ModeChanged { mode: self.mode },
        InputChange::Cleared,
    ]
}
```

---

## Phase 2: 清理废弃 InputChange 与 attachment

### Task 7: change.rs — 删除 AttachmentChanged

**Files:**
- Modify: `apps/cli/src/tui/model/input/change.rs`

- [ ] **Step 1: 删除 AttachmentChanged variant 及其在 match 中的所有引用**

从 `InputChange` enum 删除 `AttachmentChanged { count: usize }`。在 `submitted_text_from_changes` 和 `submitted_display_text_from_changes` 的 match 中删除对应分支。

- [ ] **Step 2: 更新测试中的 `InputSubmission` 构造**

所有测试中 `attachments: Vec::new()` → `images: Vec::new()`：

```rust
let changes = vec![InputChange::Submitted {
    submission: InputSubmission {
        text: "run".to_string(),
        display_text: "run".to_string(),
        images: Vec::new(),
    },
}];
```

---

### Task 8: 删除 attachment.rs

**Files:**
- Delete: `apps/cli/src/tui/model/input/attachment.rs`

- [ ] **Step 1: 删除文件**

```bash
rm apps/cli/src/tui/model/input/attachment.rs
```

（Task 1 Step 2 已从 `input.rs` 移除 `pub mod attachment;`）

---

## Phase 3: 回流路径与 ChatState 重构

> ⚠️ 从此处开始，删除旧字段后所有引用点须连续修改至编译通过。

### Task 9: executor.rs — accept_pending_clipboard_image → InsertImage

**Files:**
- Modify: `apps/cli/src/tui/effect/executor.rs:147-152`

- [ ] **Step 1: 替换方法**

```rust
fn accept_pending_clipboard_image(&mut self, img: sdk::ClipboardImageView) {
    self.handle_input_intent(
        crate::tui::model::input::intent::InputIntent::InsertImage(img),
    );
}
```

---

### Task 10: ui_event.rs — ClipboardImage → InsertImage

**Files:**
- Modify: `apps/cli/src/tui/app/update/ui_event.rs:126-131`

- [ ] **Step 1: 替换 ClipboardImage 分支**

```rust
UiEvent::ClipboardImage(img) => {
    self.handle_input_intent(
        crate::tui::model::input::intent::InputIntent::InsertImage(img),
    );
}
```

---

### Task 11: enter.rs — 从 submission 取图片

**Files:**
- Modify: `apps/cli/src/tui/app/update/enter.rs:39-51`

- [ ] **Step 1: 替换图片获取逻辑**

不再调用 `drain_pending_images()`，改为从 `InputChange::Submitted` 提取 images。修改 `update_enter`：

```rust
pub(super) fn update_enter(
    &mut self,
    ui_tx: &mpsc::Sender<UiEvent>,
    spawn_refs: &SpawnContextRefs,
) -> UpdateResult {
    let changes = self
        .model
        .input
        .apply(crate::tui::model::input::intent::InputIntent::Submit);
    let Some(submission) =
        crate::tui::model::input::change::submitted_submission_from_changes(&changes)
    else {
        return UpdateResult::none();
    };
    let input = submission.text;
    if input.is_empty() && submission.images.is_empty() {
        return UpdateResult::none();
    }
    if input.starts_with('/') {
        self.input.push_queue(input.clone());
        return UpdateResult {
            effects: Vec::new(),
            spawn_effect: None,
            pending_slash: Some(input),
        };
    }

    self.model
        .conversation
        .apply(ConversationIntent::StartChat {
            submission: input.clone(),
        });
    self.mark_output_dirty();

    let images: Vec<sdk::ToolResultImage> = submission.images.into_iter().map(Into::into).collect();
    if images.is_empty() {
        self.chat.messages.push(sdk::ChatMessage::user_text(&input));
    } else {
        self.chat
            .messages
            .push(sdk::ChatMessage::user_with_images(&input, images));
    }

    let Some(spawn_ctx) = self.build_spawn_context(ui_tx, spawn_refs) else {
        self.append_error_notice("SDK agent client is unavailable");
        return UpdateResult::none();
    };
    self.chat.clear_tool_activity();
    self.spinner_phase(crate::tui::model::runtime::spinner::SpinnerPhase::Thinking);
    self.chat.start_processing();

    UpdateResult::spawn_processing(spawn_ctx)
}
```

- [ ] **Step 2: 在 change.rs 新增辅助函数 `submitted_submission_from_changes`**

```rust
pub fn submitted_submission_from_changes(changes: &[InputChange]) -> Option<InputSubmission> {
    changes.iter().find_map(|change| match change {
        InputChange::Submitted { submission } => Some(submission.clone()),
        _ => None,
    })
}
```

---

### Task 12: chat.rs — 删除 pending_images 字段及方法

**Files:**
- Modify: `apps/cli/src/tui/app/state/chat.rs`

- [ ] **Step 1: 删除 `pending_images` 字段及 4 个方法**

从 `ChatState` struct 删除 `pub pending_images: Vec<sdk::ClipboardImageView>`。删除方法：`add_pending_image`、`clear_pending_images`、`drain_pending_images`、`pending_image_count`、`pending_images`。从 `Default::default()` 删除 `pending_images: Vec::new()`。

---

### Task 13: state/tests.rs — 删除 pending_images 测试

**Files:**
- Modify: `apps/cli/src/tui/app/state/tests.rs:44-60`

- [ ] **Step 1: 删除两个测试**

删除 `test_chat_state_pending_images_add_and_count` 和 `test_chat_state_pending_images_drain_clears`。删除 `make_clipboard_image` helper（如仅在这两个测试中使用）。保留 `test_app_model_starts_with_empty_usage_and_attachments`，但移除 `assert!(app.model.input.attachments.is_empty())` 断言（字段已删除）。

---

## Phase 4: 渲染层

### Task 14: view_model/input.rs — 删除 pending_images

**Files:**
- Modify: `apps/cli/src/tui/view_model/input.rs`

- [ ] **Step 1: 删除 `pending_images` 字段**

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputAreaViewModel {
    pub text: String,
    pub cursor: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub placeholder: Option<String>,
    pub mode_label: Option<String>,
    pub queued_hint: Option<String>,
    pub disabled_reason: Option<String>,
    pub focused: bool,
}
```

---

### Task 15: view_assembler/input.rs — 删除 pending_images 参数

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/input.rs`

- [ ] **Step 1: 更新两个函数签名，移除 `pending_images` 参数**

`assemble_from_model` 删除 `pending_images: usize` 参数。`from_document` 同样删除。`InputAreaViewModel` 构造删除 `pending_images` 字段。

```rust
impl InputViewAssembler {
    pub fn assemble_from_model(
        model: &InputModel,
        queued_count: usize,
        focused: bool,
    ) -> InputAreaViewModel {
        let placeholder = model
            .document
            .buffer
            .is_empty()
            .then(|| "输入消息...".to_string());
        let mut vm = Self::from_document(&model.document, placeholder, focused);
        vm.queued_hint = (queued_count > 0).then(|| format!("已排队 {queued_count} 条"));
        vm
    }

    pub fn from_document(
        document: &InputDocument,
        placeholder: Option<String>,
        focused: bool,
    ) -> InputAreaViewModel {
        let cursor = clamp_to_char_boundary(&document.buffer, document.cursor);
        let (cursor_row, cursor_col) = byte_cursor_to_row_col(&document.buffer, cursor);
        InputAreaViewModel {
            text: document.buffer.clone(),
            cursor,
            cursor_row,
            cursor_col,
            placeholder,
            mode_label: None,
            queued_hint: None,
            disabled_reason: None,
            focused,
        }
    }
}
```

---

### Task 16: render/input/input_area/render.rs — 标题固定

**Files:**
- Modify: `apps/cli/src/tui/render/input/input_area/render.rs`

- [ ] **Step 1: `input_block` 标题固定为 ` Input `**

```rust
fn input_block(view_model: &InputAreaViewModel) -> Block<'static> {
    let _ = view_model;
    let border_style = if view_model.focused {
        Style::default().fg(theme::ACCENT)
    } else {
        Style::default().fg(theme::BORDER)
    };
    Block::default()
        .title(" Input ")
        .borders(Borders::ALL)
        .border_style(border_style)
}
```

- [ ] **Step 2: 更新测试 `test_render_projects_pending_images_and_focus_from_vm`**

测试中 `render_vm_with_state` 签名移除 `pending_images` 参数。该测试改名为 `test_render_projects_focus_from_vm`，只验证 focus 样式：

```rust
fn render_vm_with_state(text: &str, focused: bool) -> InputAreaViewModel {
    let mut document = InputDocument::default();
    document.insert_text(text);
    InputViewAssembler::from_document(&document, None, focused)
}

#[test]
fn test_render_projects_focus_from_vm() {
    let mut input = InputArea::new();
    let area = Rect { x: 0, y: 0, width: 40, height: 3 };
    let mut buf = Buffer::empty(area);
    let vm = render_vm_with_state("hello", false);
    input.render(area, &mut buf, &vm, &InputSelectionViewState::default());
    assert_eq!(buf.cell((0, 0)).unwrap().style().fg, Some(theme::BORDER));
}
```

---

### Task 17: app.rs — 删除 pending_image_count 传参

**Files:**
- Modify: `apps/cli/src/tui/app.rs:214-219`

- [ ] **Step 1: `assemble_from_model` 调用删除 pending_images 参数**

```rust
let input_vm =
    crate::tui::view_assembler::input::InputViewAssembler::assemble_from_model(
        &self.model.input,
        0, // queued_count
        true, // focused
    );
```

搜索全仓库是否还有其他 `assemble_from_model` 调用点，同步更新。

---

## Phase 5: 命令与会话

### Task 18: slash.rs — /images、/clear-images、/clear 改读 document

**Files:**
- Modify: `apps/cli/src/tui/app/slash.rs`

- [ ] **Step 1: `/clear` 与 `CommandAction::Clear` 移除图片清理**

删除 `self.chat.clear_pending_images();` 和 `SetAttachmentCount(0)` 调用。`/clear` 改为清输入文档：

```rust
cmd if cmd == format!("/{cmd}", cmd = cmd::CLEAR) => {
    self.chat.messages.clear();
    self.handle_input_intent(
        crate::tui::model::input::intent::InputIntent::Clear,
    );
    self.output_area.clear();
    self.reset_runtime_state().await;
    self.append_system_notice("[conversation cleared]");
}
```

`CommandAction::Clear` 同样处理。

- [ ] **Step 2: `/images` 改读 document image_spans**

```rust
"/images" => {
    let spans = &self.model.input.document.image_spans;
    if spans.is_empty() {
        self.append_system_notice("No pending images.");
    } else {
        let mut text = format!("Pending images: {}", spans.len());
        for span in spans.iter() {
            text.push_str(&format!(
                "\n  {}. [Image #{}] ({} bytes)",
                span.index, span.index, span.image.final_size
            ));
        }
        self.append_system_notice(text);
    }
}
```

- [ ] **Step 3: `/clear-images` 改为清 document**

```rust
"/clear-images" => {
    self.model.input.document.image_spans.clear();
    // 重建 buffer 去除占位文本
    let text = self.model.input.document.submit_text_expanded_without_images();
    self.handle_input_intent(
        crate::tui::model::input::intent::InputIntent::ReplaceText(text),
    );
    self.append_system_notice("[pending images cleared]");
}
```

> 由于 `/clear-images` 需要移除 buffer 中所有占位文本但保留 copied text 与普通文本，在 `document.rs` 新增 `remove_all_images()` 方法：

```rust
/// 移除所有图片占位符，保留其余文本。
pub fn remove_all_images(&mut self) {
    if self.image_spans.is_empty() {
        return;
    }
    let mut spans = std::mem::take(&mut self.image_spans);
    spans.sort_by_key(|span| std::cmp::Reverse(span.start));
    for span in &spans {
        let placeholder = span.placeholder();
        if self.buffer.get(span.start..span.end) == Some(placeholder.as_str()) {
            self.buffer.replace_range(span.start..span.end, "");
        }
    }
    if self.cursor > self.buffer.len() {
        self.cursor = self.buffer.len();
    }
}
```

`/clear-images` 改用：

```rust
"/clear-images" => {
    self.model.input.document.remove_all_images();
    self.append_system_notice("[pending images cleared]");
}
```

---

### Task 19: resume.rs — 清 document 代替 clear_pending_images

**Files:**
- Modify: `apps/cli/src/tui/effect/session/resume.rs:20`

- [ ] **Step 1: 替换 `clear_pending_images` 调用**

```rust
self.chat.messages.clear();
self.handle_input_intent(
    crate::tui::model::input::intent::InputIntent::Clear,
);
```

---

### Task 20: coordinator.rs — 删除 AttachmentChanged 分支

**Files:**
- Modify: `apps/cli/src/tui/update/coordinator.rs:10`

- [ ] **Step 1: 从 match 中删除 `AttachmentChanged` 分支**

```rust
pub fn effects_for_input_change(change: &InputChange) -> Vec<Effect> {
    match change {
        InputChange::TextChanged { .. }
        | InputChange::CursorMoved { .. }
        | InputChange::CompletionChanged { .. }
        | InputChange::HistorySelected { .. }
        | InputChange::ModeChanged { .. }
        | InputChange::Submitted { .. }
        | InputChange::Cleared => vec![Effect::RequestRender],
    }
}
```

测试中 `InputSubmission` 构造同步更新 `attachments` → `images`。

---

### Task 21: adapter/input.rs — 测试更新

**Files:**
- Modify: `apps/cli/src/tui/adapter/input.rs`

- [ ] **Step 1: 所有测试中 `InputSubmission` 构造 `attachments: Vec::new()` → `images: Vec::new()`**

---

### Task 22: input_model_tests.rs — 测试更新

**Files:**
- Modify: `apps/cli/src/tui/model/input/input_model_tests.rs`

- [ ] **Step 1: 全局搜索 `attachments` / `SetAttachmentCount` / `AttachmentChanged` 并更新**

读取文件后按实际内容更新：移除对已删除字段/intent/change 的引用。

---

## Phase 6: 编译验证

### Task 23: 全量编译 + clippy + 测试

- [ ] **Step 1: 编译**

```bash
cargo build -p aemeath-cli 2>&1 | head -80
```

预期：0 errors。如有编译错误，逐一修复遗漏的引用点。

- [ ] **Step 2: clippy**

```bash
cargo clippy -p aemeath-cli --all-targets 2>&1 | grep -E "^error|^warning" | head -30
```

预期：0 errors, 0 warnings。

- [ ] **Step 3: 测试**

```bash
cargo test -p aemeath-cli 2>&1 | tail -40
```

预期：全部 PASS。

- [ ] **Step 4: 提交**

```bash
git add -A
git commit -m "feat(tui): 图片以 [Image #N] 占位符纳入 InputDocument 单一真相 (#279)"
```

---

## Self-Review

### 1. Spec coverage
- ✅ 图片在输入内容区可见 → `[Image #N]` 占位在 buffer，textarea 自然渲染
- ✅ 可删除（原子） → `atomic_span_for_backward_delete` 覆盖 image span
- ✅ 与文本混合排列 → span 偏移维护保证插入/删除正确
- ✅ 提交时剔除占位、发送图片数据 → `submit_text()` + `drain_images()`
- ✅ 标题移除 image 计数 → Task 16
- ✅ 移除双真相 → 删除 `ChatState.pending_images`
- ✅ `/images` `/clear-images` 可用 → Task 18

### 2. Placeholder scan
- Task 22 标注"读取文件后按实际内容更新"——这是必要的，因为无法在写计划时预知每个测试的确切内容。执行时必须先 Read 文件。

### 3. Type consistency
- `ImageSpan::new(index, image, start, end)` — 全计划一致
- `insert_image(image: ClipboardImageView)` — 一致
- `submit_text()` / `drain_images()` / `remove_all_images()` — 一致
- `InputSubmission.images: Vec<sdk::ClipboardImageView>` — 全计划一致
- `InputIntent::InsertImage(sdk::ClipboardImageView)` — 一致
