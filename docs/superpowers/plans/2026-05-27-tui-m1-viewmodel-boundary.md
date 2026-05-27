# TUI M1 ViewModel Boundary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立最小 `ViewModel / ViewAssembler / ViewState / ViewModelDirty` 边界，让 output/status/input render 先能消费结构化显示模型。

**Architecture:** 保留现有 `OutputArea`、`StatusBar`、`InputArea` 行为，通过 adapter 式 ViewAssembler 从现有 state 组装 ViewModel。第一阶段不迁移 tool matching，也不改 Agent 协议，只建立新边界并接入轻量测试。

**Tech Stack:** Rust 2021、ratatui、现有 `apps/cli` crate、`cargo test -p cli`、`cargo check -p cli`。

---

## File Structure

- Create: `apps/cli/src/tui/view_model/mod.rs` — ViewModel 模块出口。
- Create: `apps/cli/src/tui/view_model/style.rs` — render 无关的语义样式。
- Create: `apps/cli/src/tui/view_model/output.rs` — OutputViewModel 和 block 类型。
- Create: `apps/cli/src/tui/view_model/status.rs` — StatusLineViewModel 和 segment 类型。
- Create: `apps/cli/src/tui/view_model/input.rs` — InputAreaViewModel。
- Create: `apps/cli/src/tui/view_model/dialog.rs` — DialogViewModel。
- Create: `apps/cli/src/tui/view_state/mod.rs` — ViewState 模块出口。
- Create: `apps/cli/src/tui/view_state/output.rs` — output scroll/follow/collapse 的显示状态。
- Create: `apps/cli/src/tui/view_state/input.rs` — input 纯显示状态。
- Create: `apps/cli/src/tui/view_state/layout.rs` — terminal/layout snapshot。
- Create: `apps/cli/src/tui/view_state/animation.rs` — spinner/cursor blink frame。
- Create: `apps/cli/src/tui/view_assembler/mod.rs` — ViewAssembler 模块出口。
- Create: `apps/cli/src/tui/view_assembler/output.rs` — 从现有 `OutputArea` 组装 OutputViewModel。
- Create: `apps/cli/src/tui/view_assembler/status.rs` — 从现有 App/StatusBar 相关状态组装 StatusLineViewModel。
- Create: `apps/cli/src/tui/view_assembler/input.rs` — 从现有 `InputArea` 组装 InputAreaViewModel。
- Create: `apps/cli/src/tui/view_assembler/dialog.rs` — 从现有 ask-user/diagnostic 状态组装 DialogViewModel。
- Modify: `apps/cli/src/tui/mod.rs` — 导出新模块。
- Modify: `apps/cli/src/tui/core/state/mod.rs` — 增加 `ViewModelDirty`，或在新模块中定义后被 AppState 持有。

## Task 1: Add base ViewModel types

**Files:**
- Create: `apps/cli/src/tui/view_model/style.rs`
- Create: `apps/cli/src/tui/view_model/output.rs`
- Create: `apps/cli/src/tui/view_model/status.rs`
- Create: `apps/cli/src/tui/view_model/input.rs`
- Create: `apps/cli/src/tui/view_model/dialog.rs`
- Create: `apps/cli/src/tui/view_model/mod.rs`
- Modify: `apps/cli/src/tui/mod.rs`

- [ ] **Step 1: Create failing module visibility test**

Add this test at the end of `apps/cli/src/tui/view_model/mod.rs` after creating the file:

```rust
#[cfg(test)]
mod tests {
    use super::output::{OutputBlockView, OutputViewModel, ToolCallBlockView, ToolSemanticStatus};
    use super::style::SemanticStyle;

    #[test]
    fn test_output_view_model_accepts_tool_block() {
        let block = OutputBlockView::ToolCall(ToolCallBlockView {
            key: "chat-1/turn-1/tool-1".to_string(),
            chat_id: Some("chat-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            tool_call_id: Some("tool-1".to_string()),
            title: "Read(src/main.rs)".to_string(),
            icon: "✓".to_string(),
            semantic_status: ToolSemanticStatus::Success,
            style: SemanticStyle::Success,
            args_preview: Some("src/main.rs".to_string()),
            summary: Some("读取文件".to_string()),
            activity_summary: None,
            result_summary: Some("120 lines".to_string()),
            collapsible: true,
            collapsed: false,
        });
        let model = OutputViewModel { blocks: vec![block], version: 1, follow_tail_hint: true };
        assert_eq!(model.blocks.len(), 1);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p cli tui::view_model::tests::test_output_view_model_accepts_tool_block
```

Expected: FAIL because `view_model` module/types are not implemented yet.

- [ ] **Step 3: Implement minimal ViewModel files**

Create `apps/cli/src/tui/view_model/style.rs`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticStyle {
    Normal,
    Muted,
    Running,
    Success,
    Error,
    Warning,
    Accent,
}
```

Create `apps/cli/src/tui/view_model/output.rs`:

```rust
use super::style::SemanticStyle;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputViewModel {
    pub blocks: Vec<OutputBlockView>,
    pub version: u64,
    pub follow_tail_hint: bool,
}

impl Default for OutputViewModel {
    fn default() -> Self {
        Self { blocks: Vec::new(), version: 0, follow_tail_hint: true }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputBlockView {
    UserMessage(TextBlockView),
    AssistantMessage(TextBlockView),
    ToolCall(ToolCallBlockView),
    DiagnosticNotice(TextBlockView),
    SystemNotice(TextBlockView),
    Separator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextBlockView {
    pub key: String,
    pub text: String,
    pub style: SemanticStyle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallBlockView {
    pub key: String,
    pub chat_id: Option<String>,
    pub turn_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub title: String,
    pub icon: String,
    pub semantic_status: ToolSemanticStatus,
    pub style: SemanticStyle,
    pub args_preview: Option<String>,
    pub summary: Option<String>,
    pub activity_summary: Option<String>,
    pub result_summary: Option<String>,
    pub collapsible: bool,
    pub collapsed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolSemanticStatus {
    Pending,
    Running,
    Success,
    Error,
    Cancelled,
    Orphaned,
}
```

Create `apps/cli/src/tui/view_model/status.rs`:

```rust
use super::style::SemanticStyle;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StatusLineViewModel {
    pub left: Vec<StatusSegment>,
    pub center: Vec<StatusSegment>,
    pub right: Vec<StatusSegment>,
    pub severity: StatusSeverity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusSegment {
    pub key: String,
    pub text: String,
    pub style: SemanticStyle,
    pub priority: u8,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusSeverity {
    #[default]
    Normal,
    Info,
    Warning,
    Error,
}
```

Create `apps/cli/src/tui/view_model/input.rs`:

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputAreaViewModel {
    pub text: String,
    pub cursor: usize,
    pub placeholder: Option<String>,
    pub mode_label: Option<String>,
    pub queued_hint: Option<String>,
    pub disabled_reason: Option<String>,
}
```

Create `apps/cli/src/tui/view_model/dialog.rs`:

```rust
use super::status::StatusSeverity;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogViewModel {
    pub kind: DialogKind,
    pub title: String,
    pub body: String,
    pub actions: Vec<DialogActionViewModel>,
    pub default_action: Option<String>,
    pub severity: StatusSeverity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogKind {
    Permission,
    HookBlocked,
    Error,
    Confirmation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DialogActionViewModel {
    pub id: String,
    pub label: String,
}
```

Create `apps/cli/src/tui/view_model/mod.rs`:

```rust
pub mod dialog;
pub mod input;
pub mod output;
pub mod status;
pub mod style;

pub use dialog::{DialogActionViewModel, DialogKind, DialogViewModel};
pub use input::InputAreaViewModel;
pub use output::{OutputBlockView, OutputViewModel, TextBlockView, ToolCallBlockView, ToolSemanticStatus};
pub use status::{StatusLineViewModel, StatusSegment, StatusSeverity};
pub use style::SemanticStyle;

#[cfg(test)]
mod tests {
    use super::output::{OutputBlockView, OutputViewModel, ToolCallBlockView, ToolSemanticStatus};
    use super::style::SemanticStyle;

    #[test]
    fn test_output_view_model_accepts_tool_block() {
        let block = OutputBlockView::ToolCall(ToolCallBlockView {
            key: "chat-1/turn-1/tool-1".to_string(),
            chat_id: Some("chat-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            tool_call_id: Some("tool-1".to_string()),
            title: "Read(src/main.rs)".to_string(),
            icon: "✓".to_string(),
            semantic_status: ToolSemanticStatus::Success,
            style: SemanticStyle::Success,
            args_preview: Some("src/main.rs".to_string()),
            summary: Some("读取文件".to_string()),
            activity_summary: None,
            result_summary: Some("120 lines".to_string()),
            collapsible: true,
            collapsed: false,
        });
        let model = OutputViewModel { blocks: vec![block], version: 1, follow_tail_hint: true };
        assert_eq!(model.blocks.len(), 1);
    }
}
```

Modify `apps/cli/src/tui/mod.rs`:

```rust
pub mod completion;
pub mod core;
pub mod display;
pub mod input;
pub mod output_area;
pub mod session;
pub mod view_model;

pub use self::core::App;
pub use self::display::status_bar::StatusBar;
pub use self::input::input_area::InputArea;
pub use self::output_area::OutputArea;
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p cli tui::view_model::tests::test_output_view_model_accepts_tool_block
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/mod.rs apps/cli/src/tui/view_model
git commit -m "feat: add TUI view model types"
```

## Task 2: Add ViewState and ViewModelDirty

**Files:**
- Create: `apps/cli/src/tui/view_state/mod.rs`
- Create: `apps/cli/src/tui/view_state/output.rs`
- Create: `apps/cli/src/tui/view_state/input.rs`
- Create: `apps/cli/src/tui/view_state/layout.rs`
- Create: `apps/cli/src/tui/view_state/animation.rs`
- Modify: `apps/cli/src/tui/mod.rs`

- [ ] **Step 1: Write failing dirty flag test**

Create `apps/cli/src/tui/view_state/mod.rs` with this test first:

```rust
#[cfg(test)]
mod tests {
    use super::ViewModelDirty;

    #[test]
    fn test_view_model_dirty_tracks_and_clears_output() {
        let mut dirty = ViewModelDirty::default();
        assert!(!dirty.output);
        dirty.mark_output();
        assert!(dirty.output);
        dirty.clear_output();
        assert!(!dirty.output);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::view_state::tests::test_view_model_dirty_tracks_and_clears_output
```

Expected: FAIL because `ViewModelDirty` is not defined.

- [ ] **Step 3: Implement ViewState files**

Create `apps/cli/src/tui/view_state/output.rs`:

```rust
use std::collections::HashSet;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputViewState {
    pub scroll_offset: usize,
    pub follow_tail: bool,
    pub collapsed_blocks: HashSet<String>,
    pub selected_text_range: Option<SelectedTextRange>,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedTextRange {
    pub start_block_key: String,
    pub start_offset: usize,
    pub end_block_key: String,
    pub end_offset: usize,
}
```

Create `apps/cli/src/tui/view_state/input.rs`:

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputViewState {
    pub cursor_blink_visible: bool,
    pub completion_selected_index: Option<usize>,
    pub version: u64,
}
```

Create `apps/cli/src/tui/view_state/layout.rs`:

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LayoutViewState {
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub version: u64,
}
```

Create `apps/cli/src/tui/view_state/animation.rs`:

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AnimationViewState {
    pub spinner_frame: u64,
    pub cursor_blink_frame: u64,
    pub version: u64,
}
```

Create `apps/cli/src/tui/view_state/mod.rs`:

```rust
pub mod animation;
pub mod input;
pub mod layout;
pub mod output;

pub use animation::AnimationViewState;
pub use input::InputViewState;
pub use layout::LayoutViewState;
pub use output::{OutputViewState, SelectedTextRange};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AppViewState {
    pub output: OutputViewState,
    pub input: InputViewState,
    pub layout: LayoutViewState,
    pub animation: AnimationViewState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewModelDirty {
    pub output: bool,
    pub status: bool,
    pub input: bool,
    pub dialog: bool,
}

impl ViewModelDirty {
    pub fn mark_all(&mut self) {
        self.output = true;
        self.status = true;
        self.input = true;
        self.dialog = true;
    }

    pub fn mark_output(&mut self) { self.output = true; }
    pub fn mark_status(&mut self) { self.status = true; }
    pub fn mark_input(&mut self) { self.input = true; }
    pub fn mark_dialog(&mut self) { self.dialog = true; }

    pub fn clear_output(&mut self) { self.output = false; }
    pub fn clear_status(&mut self) { self.status = false; }
    pub fn clear_input(&mut self) { self.input = false; }
    pub fn clear_dialog(&mut self) { self.dialog = false; }
}

#[cfg(test)]
mod tests {
    use super::ViewModelDirty;

    #[test]
    fn test_view_model_dirty_tracks_and_clears_output() {
        let mut dirty = ViewModelDirty::default();
        assert!(!dirty.output);
        dirty.mark_output();
        assert!(dirty.output);
        dirty.clear_output();
        assert!(!dirty.output);
    }
}
```

Modify `apps/cli/src/tui/mod.rs` to include:

```rust
pub mod view_state;
```

- [ ] **Step 4: Run test**

```bash
cargo test -p cli tui::view_state::tests::test_view_model_dirty_tracks_and_clears_output
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/mod.rs apps/cli/src/tui/view_state
git commit -m "feat: add TUI view state and dirty flags"
```

## Task 3: Add adapter-style ViewAssemblers

**Files:**
- Create: `apps/cli/src/tui/view_assembler/mod.rs`
- Create: `apps/cli/src/tui/view_assembler/output.rs`
- Create: `apps/cli/src/tui/view_assembler/status.rs`
- Create: `apps/cli/src/tui/view_assembler/input.rs`
- Create: `apps/cli/src/tui/view_assembler/dialog.rs`
- Modify: `apps/cli/src/tui/mod.rs`

- [ ] **Step 1: Write failing assembler test**

Create `apps/cli/src/tui/view_assembler/output.rs` with this test:

```rust
#[cfg(test)]
mod tests {
    use crate::tui::output_area::{LineStyle, OutputArea};

    use super::OutputViewAssembler;

    #[test]
    fn test_output_assembler_converts_existing_lines_to_blocks() {
        let mut output = OutputArea::new();
        output.push_system("hello");
        let vm = OutputViewAssembler::assemble_from_output_area(&output, 1);
        assert_eq!(vm.version, 1);
        assert_eq!(vm.blocks.len(), 1);
        assert!(matches!(output.lines.front().map(|line| line.style), Some(LineStyle::System)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p cli tui::view_assembler::output::tests::test_output_assembler_converts_existing_lines_to_blocks
```

Expected: FAIL because assembler module is missing.

- [ ] **Step 3: Implement minimal assemblers**

Create `apps/cli/src/tui/view_assembler/output.rs`:

```rust
use crate::tui::output_area::{LineStyle, OutputArea};
use crate::tui::view_model::{OutputBlockView, OutputViewModel, SemanticStyle, TextBlockView};

pub struct OutputViewAssembler;

impl OutputViewAssembler {
    pub fn assemble_from_output_area(output: &OutputArea, version: u64) -> OutputViewModel {
        let blocks = output
            .lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let style = match line.style {
                    LineStyle::Error | LineStyle::ToolCallError => SemanticStyle::Error,
                    LineStyle::ToolCallSuccess => SemanticStyle::Success,
                    LineStyle::ToolCallRunning => SemanticStyle::Running,
                    LineStyle::System | LineStyle::Thinking => SemanticStyle::Muted,
                    _ => SemanticStyle::Normal,
                };
                OutputBlockView::SystemNotice(TextBlockView {
                    key: format!("legacy-line-{idx}"),
                    text: line.content.clone(),
                    style,
                })
            })
            .collect();
        OutputViewModel { blocks, version, follow_tail_hint: output.auto_scroll }
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::output_area::{LineStyle, OutputArea};

    use super::OutputViewAssembler;

    #[test]
    fn test_output_assembler_converts_existing_lines_to_blocks() {
        let mut output = OutputArea::new();
        output.push_system("hello");
        let vm = OutputViewAssembler::assemble_from_output_area(&output, 1);
        assert_eq!(vm.version, 1);
        assert_eq!(vm.blocks.len(), 1);
        assert!(matches!(output.lines.front().map(|line| line.style), Some(LineStyle::System)));
    }
}
```

Create `apps/cli/src/tui/view_assembler/status.rs`:

```rust
use crate::tui::view_model::{SemanticStyle, StatusLineViewModel, StatusSegment};

pub struct StatusViewAssembler;

impl StatusViewAssembler {
    pub fn assemble_basic(model_id: Option<&str>, cwd: Option<&str>) -> StatusLineViewModel {
        let mut vm = StatusLineViewModel::default();
        if let Some(model_id) = model_id {
            vm.left.push(StatusSegment { key: "model".to_string(), text: model_id.to_string(), style: SemanticStyle::Accent, priority: 10 });
        }
        if let Some(cwd) = cwd {
            vm.right.push(StatusSegment { key: "cwd".to_string(), text: cwd.to_string(), style: SemanticStyle::Muted, priority: 20 });
        }
        vm
    }
}
```

Create `apps/cli/src/tui/view_assembler/input.rs`:

```rust
use crate::tui::view_model::InputAreaViewModel;

pub struct InputViewAssembler;

impl InputViewAssembler {
    pub fn assemble_text(text: &str, cursor: usize) -> InputAreaViewModel {
        InputAreaViewModel { text: text.to_string(), cursor, placeholder: None, mode_label: None, queued_hint: None, disabled_reason: None }
    }
}
```

Create `apps/cli/src/tui/view_assembler/dialog.rs`:

```rust
use crate::tui::view_model::DialogViewModel;

pub struct DialogViewAssembler;

impl DialogViewAssembler {
    pub fn none() -> Option<DialogViewModel> { None }
}
```

Create `apps/cli/src/tui/view_assembler/mod.rs`:

```rust
pub mod dialog;
pub mod input;
pub mod output;
pub mod status;

pub use dialog::DialogViewAssembler;
pub use input::InputViewAssembler;
pub use output::OutputViewAssembler;
pub use status::StatusViewAssembler;
```

Modify `apps/cli/src/tui/mod.rs` to include:

```rust
pub mod view_assembler;
```

- [ ] **Step 4: Run assembler test and full cli check**

```bash
cargo test -p cli tui::view_assembler::output::tests::test_output_assembler_converts_existing_lines_to_blocks
cargo check -p cli
```

Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/mod.rs apps/cli/src/tui/view_assembler
git commit -m "feat: add TUI view assemblers"
```

## Task 4: Add M1 architecture guard seed

**Files:**
- Modify: `.agents/hooks/check-tui-tea-purity.sh`
- Modify: `.agents/hooks/check-architecture-guards.sh`

- [ ] **Step 1: Add failing guard expectation**

Run existing guard first:

```bash
.agents/hooks/check-architecture-guards.sh
```

Expected before modification: PASS or current repository-known failures unrelated to M1. Record output in the implementation notes.

- [ ] **Step 2: Extend TUI purity target directories**

Modify `.agents/hooks/check-tui-tea-purity.sh` so its checked directories include both legacy and new pure layers. Add this shell snippet near the existing target directory definition:

```bash
TUI_PURE_DIRS=(
  "apps/cli/src/tui/core"
  "apps/cli/src/tui/model"
  "apps/cli/src/tui/view_assembler"
  "apps/cli/src/tui/view_model"
)
```

Then iterate only over directories that exist:

```bash
for dir in "${TUI_PURE_DIRS[@]}"; do
  [ -d "$dir" ] || continue
  while IFS= read -r file; do
    check_file "$file"
  done < <(find "$dir" -name '*.rs' -type f | sort)
done
```

If the script currently uses a single hard-coded path, replace that path loop with the above. Keep existing exempt file logic intact.

- [ ] **Step 3: Run guard**

```bash
.agents/hooks/check-tui-tea-purity.sh
.agents/hooks/check-architecture-guards.sh
```

Expected: PASS.

- [ ] **Step 4: Run Rust validation**

```bash
cargo test -p cli tui::view_model::tests::test_output_view_model_accepts_tool_block tui::view_state::tests::test_view_model_dirty_tracks_and_clears_output tui::view_assembler::output::tests::test_output_assembler_converts_existing_lines_to_blocks
cargo check -p cli
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add .agents/hooks/check-tui-tea-purity.sh .agents/hooks/check-architecture-guards.sh
git commit -m "chore: extend TUI architecture guard targets"
```

## Final verification

Run:

```bash
cargo test -p cli
cargo check -p cli
.agents/hooks/check-architecture-guards.sh
```

Expected: all PASS.

Plan complete when M1 introduces the boundary modules without changing existing UI behavior.
