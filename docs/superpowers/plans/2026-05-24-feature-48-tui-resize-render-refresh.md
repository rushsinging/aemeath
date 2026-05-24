# Feature #48 TUI Resize Render Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make TUI terminal resize events explicitly refresh layout-sensitive state so output wrapping/cache, scroll, selection, input width, and status rendering stay consistent after window size changes.

**Architecture:** Use the confirmed centralized resize design: `Msg::Resize { width, height }` enters `App::handle_resize()` as the only update-stage resize entrypoint. `App` records `TerminalSize`, delegates output cache/scroll/selection handling to `OutputArea::handle_resize()`, delegates input width/selection handling to `InputArea::handle_resize()`, and leaves actual ratatui layout as render-time truth.

**Tech Stack:** Rust 2024, crossterm event loop, ratatui rendering, tui-textarea, existing `cargo test -p aemeath-cli` verification and architecture guard hooks.

---

## Files and responsibilities

- Modify `cli/src/tui/app/msg.rs`
  - Change `Msg::Resize` from unit variant to `Resize { width: u16, height: u16 }`.

- Modify `cli/src/tui/app/run_loop.rs`
  - Preserve crossterm resize dimensions and pass them through `Msg::Resize { width, height }`.

- Modify `cli/src/tui/app/mod.rs`
  - Add `TerminalSize` value object.
  - Add `App.last_terminal_size: Option<TerminalSize>`.
  - Initialize the new field in `App::new()`.

- Create `cli/src/tui/app/resize.rs`
  - Implement `App::handle_resize(width, height)`.
  - Keep resize logic out of `update.rs` to prevent that file from growing.
  - Add focused tests for duplicate resize and state update.

- Modify `cli/src/tui/app/update.rs`
  - Route `Msg::Resize { width, height }` to `self.handle_resize(width, height)` and return `Cmd::None`.

- Modify `cli/src/tui/output_area/rendered_cache.rs`
  - Add test-only visibility helpers to assert invalidation state without exposing internals in production.
  - Keep production API small.

- Create `cli/src/tui/output_area/resize.rs`
  - Implement `OutputArea::handle_resize(width, visible_height_hint)`.
  - Invalidate render cache only when width changes.
  - Clamp `scroll_offset` for new visible height.
  - Clear active output selection on resize.
  - Add unit tests for each behavior.

- Modify `cli/src/tui/output_area/mod.rs`
  - Register `mod resize;`.

- Create `cli/src/tui/input_area/resize.rs`
  - Implement `InputArea::handle_resize(width)`.
  - Set `content_width` from the passed render/input width.
  - Clear active input selection on resize.
  - Add unit tests.

- Modify `cli/src/tui/input_area.rs`
  - Register `mod resize;`.

- Modify `docs/feature/active.md`
  - After implementation is complete, update #48 status from `设计中` to `待确认` and summarize implemented behavior.

---

### Task 1: Carry resize dimensions through the TEA message pipeline

**Files:**
- Modify: `cli/src/tui/app/msg.rs`
- Modify: `cli/src/tui/app/run_loop.rs`
- Modify: `cli/src/tui/app/update.rs`

- [ ] **Step 1: Change `Msg::Resize` to carry dimensions**

Edit `cli/src/tui/app/msg.rs`.

Replace:

```rust
    Resize,
```

with:

```rust
    Resize { width: u16, height: u16 },
```

- [ ] **Step 2: Pass crossterm resize dimensions into the message**

Edit `cli/src/tui/app/run_loop.rs`.

Replace:

```rust
                            Event::Resize(_, _) => Some(Msg::Resize),
```

with:

```rust
                            Event::Resize(width, height) => Some(Msg::Resize { width, height }),
```

- [ ] **Step 3: Temporarily update the match arm so the code compiles before handler work**

Edit `cli/src/tui/app/update.rs`.

Replace:

```rust
              Msg::Resize => UpdateResult {
                  cmd: Cmd::None,
                  pending_slash: None,
              },
```

with:

```rust
              Msg::Resize { .. } => UpdateResult {
                  cmd: Cmd::None,
                  pending_slash: None,
              },
```

- [ ] **Step 4: Verify the message pipeline compiles**

Run:

```bash
cargo check -p aemeath-cli
```

Expected: command exits successfully.

- [ ] **Step 5: Commit the pipeline change**

Run:

```bash
git add cli/src/tui/app/msg.rs cli/src/tui/app/run_loop.rs cli/src/tui/app/update.rs
git commit -m "feat: 携带 TUI resize 尺寸 (refs #48)" \
  -m "- 将 Msg::Resize 改为携带 width/height" \
  -m "- 从 crossterm resize 事件保留终端尺寸" \
  -m "Co-Authored-By: Aemeath (LiteLLM/gpt-5.5) <github:rushsinging/aemeath>"
```

---

### Task 2: Add App resize state and centralized handler

**Files:**
- Modify: `cli/src/tui/app/mod.rs`
- Create: `cli/src/tui/app/resize.rs`
- Modify: `cli/src/tui/app/update.rs`

- [ ] **Step 1: Add the resize module declaration**

Edit `cli/src/tui/app/mod.rs` near the other module declarations.

Add:

```rust
mod resize;
```

- [ ] **Step 2: Add `TerminalSize` near the `App` struct**

Edit `cli/src/tui/app/mod.rs` before `pub struct App`.

Add:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalSize {
    pub width: u16,
    pub height: u16,
}
```

- [ ] **Step 3: Add `last_terminal_size` to `App`**

Edit `cli/src/tui/app/mod.rs` inside `pub struct App` after the rect fields:

```rust
    pub output_area_rect: Rect,
    pub input_area_rect: Rect,
    pub status_bar_rect: Rect,
```

Add:

```rust
    pub last_terminal_size: Option<TerminalSize>,
```

- [ ] **Step 4: Initialize `last_terminal_size` in `App::new()`**

Edit `cli/src/tui/app/mod.rs` inside the `Self { ... }` initializer after `status_bar_rect: Rect::default(),`.

Add:

```rust
              last_terminal_size: None,
```

- [ ] **Step 5: Create the failing App resize tests**

Create `cli/src/tui/app/resize.rs` with this content:

```rust
use super::{App, TerminalSize};

impl App {
    pub(crate) fn handle_resize(&mut self, width: u16, height: u16) {
        let new_size = TerminalSize { width, height };
        if self.last_terminal_size == Some(new_size) {
            return;
        }
        self.last_terminal_size = Some(new_size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_resize_records_terminal_size() {
        let mut app = App::new(
            "session-1".to_string(),
            std::path::PathBuf::from("/tmp"),
            "model".to_string(),
        );

        app.handle_resize(120, 40);

        assert_eq!(
            app.last_terminal_size,
            Some(TerminalSize {
                width: 120,
                height: 40,
            })
        );
    }

    #[test]
    fn test_handle_resize_ignores_duplicate_size() {
        let mut app = App::new(
            "session-1".to_string(),
            std::path::PathBuf::from("/tmp"),
            "model".to_string(),
        );

        app.handle_resize(120, 40);
        app.output_area.scroll_offset = 99;
        app.handle_resize(120, 40);

        assert_eq!(app.output_area.scroll_offset, 99);
    }
}
```

- [ ] **Step 6: Route `Msg::Resize` to the handler**

Edit `cli/src/tui/app/update.rs`.

Replace:

```rust
              Msg::Resize { .. } => UpdateResult {
                  cmd: Cmd::None,
                  pending_slash: None,
              },
```

with:

```rust
              Msg::Resize { width, height } => {
                  self.handle_resize(width, height);
                  UpdateResult {
                      cmd: Cmd::None,
                      pending_slash: None,
                  }
              }
```

- [ ] **Step 7: Run the App resize tests**

Run:

```bash
cargo test -p aemeath-cli tui::app::resize::tests -- --nocapture
```

Expected: both tests pass.

- [ ] **Step 8: Run the package check**

Run:

```bash
cargo check -p aemeath-cli
```

Expected: command exits successfully.

- [ ] **Step 9: Commit the App resize handler**

Run:

```bash
git add cli/src/tui/app/mod.rs cli/src/tui/app/resize.rs cli/src/tui/app/update.rs
git commit -m "feat: 集中处理 TUI resize 状态 (refs #48)" \
  -m "- 新增 TerminalSize 和 App.last_terminal_size" \
  -m "- 通过 App::handle_resize 统一处理 resize 消息" \
  -m "Co-Authored-By: Aemeath (LiteLLM/gpt-5.5) <github:rushsinging/aemeath>"
```

---

### Task 3: Implement OutputArea resize cache invalidation and scroll/selection reset

**Files:**
- Modify: `cli/src/tui/output_area/mod.rs`
- Modify: `cli/src/tui/output_area/rendered_cache.rs`
- Create: `cli/src/tui/output_area/resize.rs`
- Modify: `cli/src/tui/app/resize.rs`

- [ ] **Step 1: Register the output resize module**

Edit `cli/src/tui/output_area/mod.rs` near the existing module list.

Add:

```rust
mod resize;
```

- [ ] **Step 2: Add test-only helpers to `RenderedCache`**

Edit `cli/src/tui/output_area/rendered_cache.rs` inside `impl RenderedCache` after `pub fn invalidate(&mut self)`.

Add:

```rust
    #[cfg(test)]
    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[cfg(test)]
    pub(crate) fn mark_clean_for_test(&mut self, width: usize) {
        self.cached_width = width;
        self.dirty = false;
    }
```

- [ ] **Step 3: Create OutputArea resize implementation and tests**

Create `cli/src/tui/output_area/resize.rs` with this content:

```rust
use super::OutputArea;

impl OutputArea {
    pub(crate) fn handle_resize(&mut self, width: u16, visible_height_hint: u16) {
        let new_term_width = (width as usize).saturating_sub(2);
        if new_term_width != self.term_width {
            self.term_width = new_term_width;
            self.rendered_cache.invalidate();
        }

        self.last_visible_height = visible_height_hint as usize;
        self.clamp_scroll_for_visible_height(visible_height_hint as usize);

        if self.is_selecting
            || self.selection_start.is_some()
            || self.selection_end.is_some()
        {
            self.clear_selection();
        }
    }

    fn clamp_scroll_for_visible_height(&mut self, visible_height: usize) {
        let total_lines = self.lines.len();
        let max_offset = total_lines.saturating_sub(visible_height);
        self.scroll_offset = self.scroll_offset.min(max_offset);
        if self.scroll_offset == 0 {
            self.auto_scroll = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::output_area::{LineStyle, OutputLine};
    use aemeath_core::string_idx::CharIdx;

    fn output_with_lines(count: usize) -> OutputArea {
        let mut output = OutputArea::new();
        output.lines.clear();
        for idx in 0..count {
            output.push_line(OutputLine {
                content: format!("line {idx}"),
                style: LineStyle::Assistant,
                ..Default::default()
            });
        }
        output
    }

    #[test]
    fn test_handle_resize_invalidates_cache_when_width_changes() {
        let mut output = output_with_lines(3);
        output.term_width = 78;
        output.rendered_cache.mark_clean_for_test(78);

        output.handle_resize(100, 20);

        assert_eq!(output.term_width, 98);
        assert!(output.rendered_cache.is_dirty());
    }

    #[test]
    fn test_handle_resize_keeps_cache_when_width_is_unchanged() {
        let mut output = output_with_lines(3);
        output.term_width = 98;
        output.rendered_cache.mark_clean_for_test(98);

        output.handle_resize(100, 20);

        assert_eq!(output.term_width, 98);
        assert!(!output.rendered_cache.is_dirty());
    }

    #[test]
    fn test_handle_resize_clamps_scroll_offset_to_visible_height() {
        let mut output = output_with_lines(10);
        output.auto_scroll = false;
        output.scroll_offset = 99;

        output.handle_resize(80, 4);

        assert_eq!(output.scroll_offset, 6);
        assert!(!output.auto_scroll);
    }

    #[test]
    fn test_handle_resize_restores_auto_scroll_when_offset_reaches_bottom() {
        let mut output = output_with_lines(3);
        output.auto_scroll = false;
        output.scroll_offset = 10;

        output.handle_resize(80, 10);

        assert_eq!(output.scroll_offset, 0);
        assert!(output.auto_scroll);
    }

    #[test]
    fn test_handle_resize_clears_active_selection() {
        let mut output = output_with_lines(3);
        output.is_selecting = true;
        output.selection_start = Some((0, CharIdx::new(0)));
        output.selection_end = Some((1, CharIdx::new(1)));

        output.handle_resize(80, 10);

        assert!(!output.is_selecting());
        assert!(output.selection_start.is_none());
        assert!(output.selection_end.is_none());
    }
}
```

- [ ] **Step 4: Delegate from `App::handle_resize()` to `OutputArea`**

Edit `cli/src/tui/app/resize.rs`.

Replace the current `handle_resize` implementation with:

```rust
impl App {
    pub(crate) fn handle_resize(&mut self, width: u16, height: u16) {
        let new_size = TerminalSize { width, height };
        if self.last_terminal_size == Some(new_size) {
            return;
        }
        self.last_terminal_size = Some(new_size);

        let visible_height_hint = self.output_area_rect.height.max(height.saturating_sub(7));
        self.output_area
            .handle_resize(width, visible_height_hint);
    }
}
```

- [ ] **Step 5: Update the duplicate App resize test to observe delegated behavior**

Edit `cli/src/tui/app/resize.rs` test `test_handle_resize_ignores_duplicate_size`.

Replace the body after `app.handle_resize(120, 40);` with:

```rust
        app.output_area.term_width = 118;
        app.output_area.scroll_offset = 99;
        app.output_area.handle_resize(80, 10);
        app.handle_resize(120, 40);

        assert_eq!(app.output_area.scroll_offset, 99);
```

This keeps the test focused on duplicate app resize not delegating again.

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test -p aemeath-cli tui::output_area::resize::tests -- --nocapture
cargo test -p aemeath-cli tui::app::resize::tests -- --nocapture
```

Expected: all focused resize tests pass.

- [ ] **Step 7: Run package check**

Run:

```bash
cargo check -p aemeath-cli
```

Expected: command exits successfully.

- [ ] **Step 8: Commit OutputArea resize support**

Run:

```bash
git add cli/src/tui/output_area/mod.rs cli/src/tui/output_area/rendered_cache.rs cli/src/tui/output_area/resize.rs cli/src/tui/app/resize.rs
git commit -m "feat: resize 时刷新 output 渲染状态 (refs #48)" \
  -m "- 宽度变化时失效 output render cache" \
  -m "- 高度变化时 clamp scroll 并清理 selection" \
  -m "Co-Authored-By: Aemeath (LiteLLM/gpt-5.5) <github:rushsinging/aemeath>"
```

---

### Task 4: Implement InputArea resize width and selection handling

**Files:**
- Modify: `cli/src/tui/input_area.rs`
- Create: `cli/src/tui/input_area/resize.rs`
- Modify: `cli/src/tui/app/resize.rs`

- [ ] **Step 1: Register the input resize module**

Edit `cli/src/tui/input_area.rs` near the existing module list.

Add:

```rust
mod resize;
```

- [ ] **Step 2: Create InputArea resize implementation and tests**

Create `cli/src/tui/input_area/resize.rs` with this content:

```rust
use super::InputArea;

impl InputArea {
    pub(crate) fn handle_resize(&mut self, width: u16) {
        self.content_width = width.saturating_sub(2);
        if self.is_selecting
            || self.selection_start.is_some()
            || self.selection_end.is_some()
        {
            self.clear_selection();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_resize_updates_content_width() {
        let mut input = InputArea::new();

        input.handle_resize(80);

        assert_eq!(input.content_width, 78);
    }

    #[test]
    fn test_handle_resize_saturates_small_width() {
        let mut input = InputArea::new();

        input.handle_resize(1);

        assert_eq!(input.content_width, 0);
    }

    #[test]
    fn test_handle_resize_clears_active_selection() {
        let mut input = InputArea::new();
        input.is_selecting = true;
        input.selection_start = Some((0, 0));
        input.selection_end = Some((0, 3));

        input.handle_resize(80);

        assert!(!input.is_selecting());
        assert!(input.selection_start.is_none());
        assert!(input.selection_end.is_none());
    }
}
```

- [ ] **Step 3: Delegate from `App::handle_resize()` to `InputArea`**

Edit `cli/src/tui/app/resize.rs`.

Inside `handle_resize`, after the `self.output_area.handle_resize(...)` call, add:

```rust
        let input_width = self.input_area_rect.width.max(width);
        self.input_area.handle_resize(input_width);
```

The complete implementation should now be:

```rust
impl App {
    pub(crate) fn handle_resize(&mut self, width: u16, height: u16) {
        let new_size = TerminalSize { width, height };
        if self.last_terminal_size == Some(new_size) {
            return;
        }
        self.last_terminal_size = Some(new_size);

        let visible_height_hint = self.output_area_rect.height.max(height.saturating_sub(7));
        self.output_area
            .handle_resize(width, visible_height_hint);

        let input_width = self.input_area_rect.width.max(width);
        self.input_area.handle_resize(input_width);
    }
}
```

- [ ] **Step 4: Add App integration test for input delegation**

Edit `cli/src/tui/app/resize.rs` tests module.

Add:

```rust
    #[test]
    fn test_handle_resize_updates_input_width() {
        let mut app = App::new(
            "session-1".to_string(),
            std::path::PathBuf::from("/tmp"),
            "model".to_string(),
        );

        app.handle_resize(80, 24);

        assert_eq!(app.input_area.content_width, 78);
    }
```

- [ ] **Step 5: Run focused tests**

Run:

```bash
cargo test -p aemeath-cli tui::input_area::resize::tests -- --nocapture
cargo test -p aemeath-cli tui::app::resize::tests -- --nocapture
```

Expected: all focused tests pass.

- [ ] **Step 6: Run package check**

Run:

```bash
cargo check -p aemeath-cli
```

Expected: command exits successfully.

- [ ] **Step 7: Commit InputArea resize support**

Run:

```bash
git add cli/src/tui/input_area.rs cli/src/tui/input_area/resize.rs cli/src/tui/app/resize.rs
git commit -m "feat: resize 时刷新 input 宽度状态 (refs #48)" \
  -m "- resize 后更新 input content_width" \
  -m "- resize 时清理 input selection，避免旧坐标错位" \
  -m "Co-Authored-By: Aemeath (LiteLLM/gpt-5.5) <github:rushsinging/aemeath>"
```

---

### Task 5: Update feature tracking and run final verification

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Update #48 active feature status**

Edit `docs/feature/active.md`.

Replace the #48 row:

```markdown
| 48 | TUI 窗口 resize 时重新计算渲染层并刷新显示层 | 高 | 设计中 | 已确认 | 拖动终端窗口大小或收到 resize 事件时，TUI 应重新计算 layout、wrap、scroll、selection、Markdown/table/code/diff 等渲染缓存，并刷新显示层；已确认采用集中式 ResizeState/LayoutSnapshot 方案。详见 [spec](specs/048-tui-resize-render-refresh.md) |
```

with:

```markdown
| 48 | TUI 窗口 resize 时重新计算渲染层并刷新显示层 | 高 | 待确认 | 已完成 | TUI resize 已接入集中式处理：Resize 消息携带终端宽高，App 记录最近尺寸并统一刷新 output cache/scroll/selection 与 input width/selection；status line 继续按 render 宽度即时重算。详见 [spec](specs/048-tui-resize-render-refresh.md) |
```

- [ ] **Step 2: Run rustfmt**

Run:

```bash
cargo fmt -p aemeath-cli --check
```

Expected: command exits successfully. If it fails, run:

```bash
cargo fmt -p aemeath-cli
```

Then verify with:

```bash
cargo fmt -p aemeath-cli --check
```

- [ ] **Step 3: Run package verification**

Run:

```bash
cargo check -p aemeath-cli
cargo test -p aemeath-cli
.agents/hooks/check-architecture-guards.sh
git diff --check
```

Expected:

- `cargo check -p aemeath-cli` exits successfully.
- `cargo test -p aemeath-cli` exits successfully.
- architecture guard exits successfully and reports allow/pass.
- `git diff --check` prints no output.

- [ ] **Step 4: Commit tracking and final verification update**

Run:

```bash
git add docs/feature/active.md
git commit -m "docs: 更新 TUI resize feature 状态 (refs #48)" \
  -m "- 标记 #48 实现完成并等待确认" \
  -m "- 记录 resize 对 output/input/status 的刷新范围" \
  -m "Co-Authored-By: Aemeath (LiteLLM/gpt-5.5) <github:rushsinging/aemeath>"
```

- [ ] **Step 5: Confirm clean worktree**

Run:

```bash
git status --short --branch
```

Expected: no modified or untracked files.

---

## Self-review

### Spec coverage

- Unique resize update entrypoint: Task 1 changes the message, Task 2 routes it through `App::handle_resize()`.
- Record latest terminal size and ignore duplicates: Task 2 adds `TerminalSize`, `last_terminal_size`, and duplicate test.
- Output cache invalidation: Task 3 adds width-aware invalidation tests and implementation.
- Output scroll clamp: Task 3 adds clamp tests and implementation.
- Output selection reset: Task 3 clears active or stale output selection.
- Input width and selection handling: Task 4 updates `content_width` and clears input selection.
- Status line render-time behavior: no code change needed; final docs record this and final verification exercises the package.
- Avoid per-frame cache rebuild: Task 3 only invalidates on width change.

### Placeholder scan

The plan contains no TBD/TODO placeholders. Every code-changing step includes exact code blocks and exact commands.

### Type consistency

- `TerminalSize` is defined in `cli/src/tui/app/mod.rs` and imported in `cli/src/tui/app/resize.rs` via `use super::{App, TerminalSize};`.
- `Msg::Resize { width, height }` is used consistently in `msg.rs`, `run_loop.rs`, and `update.rs`.
- `OutputArea::handle_resize(width, visible_height_hint)` and `InputArea::handle_resize(width)` are used from `App::handle_resize()`.
