# Feature #46 Two-Line Status Line Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the balanced two-line TUI status line selected by the user: runtime status on line 1, concise working context on line 2.

**Architecture:** Keep the feature local to the existing TUI `StatusBar` and `App::draw` layout. `StatusBar` owns a small `StatusLineContext` data object and renders two rows: row 1 preserves existing model/thinking/status/token/session data, row 2 shows `ctx`, shortened `cwd/path_base`, `root`, `main/worktree + branch`, and permission mode. Runtime context starts from the existing `App.cwd`/`set_current_dir` path and is structured so #43/#45 can later feed real `path_base`/`working_root` without changing the renderer.

**Tech Stack:** Rust, ratatui `Paragraph`/`Line`/`Span`, existing `theme` colors, `cargo test -p aemeath-cli`, `cargo check -p aemeath-cli`, `.agents/hooks/check-architecture-guards.sh`.

---

## File Structure

- Modify `cli/src/tui/status_bar.rs`
  - Add `StatusLineContext` and `WorktreeKind`.
  - Replace `current_dir: Option<String>` with structured context fields while preserving `set_current_dir()` for existing callers.
  - Split rendering into helpers so the file remains under 400 lines.
- Modify `cli/src/tui/status_bar_tests.rs`
  - Add tests for row 2 rendering, narrow truncation, selection text, and setter compatibility.
- Modify `cli/src/tui/app/mod.rs`
  - Change status bar layout height from 1 to 2.
  - Keep `status_bar_rect` pointing at the full two-line area.
- Modify `docs/feature/active.md`
  - Update #46 state from `规划完成，待实施` to `实现中` at the start, then `待确认` after implementation is verified.

## Current Code Facts

- `StatusBar::render(area, buf)` currently renders one `Line` into a one-row area.
- `App::draw()` currently uses layout constraints `[Min(10), Length(5), Length(suggestions_height), Length(1)]`; the final `Length(1)` is the status bar.
- `App::new()` calls `status_bar.set_current_dir(display_working_dir(&cwd));`, so implementation must preserve this method or update the call.
- There is no permission mode implementation in current main branch; row 2 should default to `Perm: AskMe` and expose a setter for #42.
- Grep found no current `path_base`/`working_root` runtime source in this branch, so the initial implementation should treat `cwd/path_base` and `root` as the same source unless callers set them separately later.

---

### Task 1: Add Structured Context and Pure Formatting Tests

**Files:**
- Modify: `cli/src/tui/status_bar.rs`
- Modify: `cli/src/tui/status_bar_tests.rs`

- [ ] **Step 1: Write failing tests for context formatting**

Append these tests to `cli/src/tui/status_bar_tests.rs`:

```rust
#[test]
fn test_status_line_context_defaults_to_balanced_row() {
    let mut bar = StatusBar::new();
    bar.set_model("claude-sonnet");
    bar.set_context_paths(
        "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-46-status-line/cli/src/tui",
        "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-46-status-line",
    );
    bar.set_git_context(WorktreeKind::Worktree, "feature/46-status-line");
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text(120);

    assert!(row.contains("ctx "));
    assert!(row.contains("…/feature-46-status-line/cli/src/tui"));
    assert!(row.contains("root …/feature-46-status-line"));
    assert!(row.contains("worktree:feature/46-status-line"));
    assert!(row.contains("Perm:AskMe"));
}

#[test]
fn test_status_line_context_narrow_keeps_path_branch_and_permission() {
    let mut bar = StatusBar::new();
    bar.set_context_paths(
        "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-46-status-line/cli/src/tui/app/update",
        "/Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-46-status-line",
    );
    bar.set_git_context(WorktreeKind::Worktree, "feature/46-status-line");
    bar.set_permission_mode("AllowAll");

    let row = bar.context_row_text(56);

    assert!(row.chars().count() <= 56);
    assert!(row.contains("…/update"));
    assert!(row.contains("feature/46-status-line"));
    assert!(row.contains("Perm:AllowAll"));
}

#[test]
fn test_set_current_dir_preserves_existing_callers() {
    let mut bar = StatusBar::new();

    bar.set_current_dir("aemeath");

    assert_eq!(bar.context_row_text(80), "ctx aemeath │ root aemeath │ main │ Perm:AskMe");
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-46-status-line
cargo test -p aemeath-cli status_bar -- --nocapture
```

Expected: FAIL because `WorktreeKind`, `set_context_paths`, `set_git_context`, `set_permission_mode`, and `context_row_text` do not exist.

- [ ] **Step 3: Add context types and setters**

In `cli/src/tui/status_bar.rs`, add these definitions after `StatusType`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorktreeKind {
    Main,
    Worktree,
}

impl WorktreeKind {
    fn label(self) -> &'static str {
        match self {
            WorktreeKind::Main => "main",
            WorktreeKind::Worktree => "worktree",
        }
    }
}

#[derive(Clone, Debug)]
struct StatusLineContext {
    path_base: String,
    working_root: String,
    worktree_kind: WorktreeKind,
    branch: Option<String>,
    permission_mode: String,
}

impl Default for StatusLineContext {
    fn default() -> Self {
        Self {
            path_base: String::new(),
            working_root: String::new(),
            worktree_kind: WorktreeKind::Main,
            branch: None,
            permission_mode: "AskMe".to_string(),
        }
    }
}
```

Change `StatusBar` field:

```rust
/// Current working context displayed to avoid operating in the wrong worktree
context: StatusLineContext,
```

Replace the old initialization `current_dir: None,` with:

```rust
context: StatusLineContext::default(),
```

Add these methods inside `impl StatusBar` near `set_current_dir`:

```rust
/// Set current working root display for existing callers.
pub fn set_current_dir(&mut self, dir: impl Into<String>) {
    let dir = dir.into();
    self.context.path_base = dir.clone();
    self.context.working_root = dir;
}

/// Set path_base/cwd and working_root for the two-line status context.
pub fn set_context_paths(&mut self, path_base: impl Into<String>, working_root: impl Into<String>) {
    self.context.path_base = path_base.into();
    self.context.working_root = working_root.into();
}

/// Set git checkout/worktree identity for the status context.
pub fn set_git_context(&mut self, kind: WorktreeKind, branch: impl Into<String>) {
    let branch = branch.into();
    self.context.worktree_kind = kind;
    self.context.branch = if branch.trim().is_empty() {
        None
    } else {
        Some(branch)
    };
}

/// Set permission mode text for the status context.
pub fn set_permission_mode(&mut self, mode: impl Into<String>) {
    self.context.permission_mode = mode.into();
}
```

- [ ] **Step 4: Add row formatting helpers**

Add these helper functions inside `impl StatusBar` before `render`:

```rust
pub(crate) fn context_row_text(&self, width: usize) -> String {
    let path = shorten_path(&self.context.path_base, context_path_width(width));
    let root = shorten_path(&self.context.working_root, root_path_width(width));
    let git = match &self.context.branch {
        Some(branch) if !branch.is_empty() => {
            format!("{}:{}", self.context.worktree_kind.label(), branch)
        }
        _ => self.context.worktree_kind.label().to_string(),
    };
    let full = format!(
        "ctx {} │ root {} │ {} │ Perm:{}",
        path, root, git, self.context.permission_mode
    );
    truncate_to_width(&full, width)
}
```

Add these free functions above the `#[cfg(test)]` module:

```rust
fn context_path_width(width: usize) -> usize {
    if width < 70 { 16 } else { 42 }
}

fn root_path_width(width: usize) -> usize {
    if width < 70 { 0 } else { 28 }
}

fn shorten_path(path: &str, max_chars: usize) -> String {
    if max_chars == 0 || path.is_empty() {
        return String::new();
    }
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        return normalized.chars().take(max_chars).collect();
    }
    let tail_parts = if max_chars < 18 { 1 } else { 3 };
    let tail = parts
        .iter()
        .rev()
        .take(tail_parts)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("/");
    let candidate = if parts.len() > tail_parts {
        format!("…/{tail}")
    } else {
        tail
    };
    truncate_to_width(&candidate, max_chars)
}

fn truncate_to_width(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let keep = max_chars - 1;
    let suffix: String = text
        .chars()
        .rev()
        .take(keep)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{suffix}")
}
```

- [ ] **Step 5: Update old `current_dir` references**

In `render()`, temporarily replace this old block:

```rust
// Current working root
if let Some(ref dir) = self.current_dir {
    spans.push(Span::styled(
        format!(" Dir: {} │", dir),
        Style::default().fg(theme::TEXT_MUTED),
    ));
}
```

with nothing. Context now belongs to row 2.

In `build_full_text()`, remove the old current dir block:

```rust
if let Some(ref dir) = self.current_dir {
    parts.push(format!(" Dir: {} │", dir));
}
```

- [ ] **Step 6: Run tests**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-46-status-line
cargo test -p aemeath-cli status_bar -- --nocapture
```

Expected: new context tests PASS; existing selection tests may still pass because row 1 full text is unchanged except `Dir` removal.

- [ ] **Step 7: Commit**

Run:

```bash
git add cli/src/tui/status_bar.rs cli/src/tui/status_bar_tests.rs
git commit -m "feat(#46): add status line context formatting"
```

---

### Task 2: Render Two Status Rows

**Files:**
- Modify: `cli/src/tui/status_bar.rs`
- Modify: `cli/src/tui/status_bar_tests.rs`

- [ ] **Step 1: Write failing render tests**

Append these tests to `cli/src/tui/status_bar_tests.rs`:

```rust
#[test]
fn test_status_bar_render_draws_context_on_second_row() {
    let mut bar = StatusBar::new();
    bar.set_current_dir("feature-46-status-line");
    bar.set_git_context(WorktreeKind::Worktree, "feature/46-status-line");
    let area = Rect::new(0, 0, 80, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf);

    let second_row: String = (0..80)
        .map(|x| buf.cell((x, 1)).unwrap().symbol())
        .collect();
    assert!(second_row.contains("ctx feature-46-status-line"));
    assert!(second_row.contains("worktree:feature/46-status-line"));
}

#[test]
fn test_status_bar_render_one_row_degrades_to_runtime_row() {
    let mut bar = StatusBar::new();
    bar.set_current_dir("feature-46-status-line");
    let area = Rect::new(0, 0, 60, 1);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf);

    let first_row: String = (0..60)
        .map(|x| buf.cell((x, 0)).unwrap().symbol())
        .collect();
    assert!(first_row.contains("Think:ON"));
    assert!(!first_row.contains("ctx feature-46-status-line"));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p aemeath-cli status_bar -- --nocapture
```

Expected: `test_status_bar_render_draws_context_on_second_row` FAIL because render still draws one row.

- [ ] **Step 3: Extract runtime row builder**

In `cli/src/tui/status_bar.rs`, move the current span-building body from `render()` into a new private method:

```rust
fn runtime_row_spans(&self) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    if let Some(ref model) = self.model {
        spans.push(Span::styled(
            format!(" {} ", model),
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled("│".to_string(), Style::default().fg(theme::BORDER)));
    }

    let label = if self.thinking { "ON" } else { "OFF" };
    let color = if self.thinking { theme::SUCCESS } else { theme::TEXT_DIM };
    spans.push(Span::styled(
        format!(" Think:{} │", label),
        Style::default().fg(color),
    ));

    let status_style = match self.status_type {
        StatusType::Normal => Style::default().fg(theme::TEXT),
        StatusType::Success => Style::default().fg(theme::SUCCESS),
        StatusType::Warning => Style::default().fg(theme::WARNING),
    };
    spans.push(Span::styled(format!(" {} ", self.status), status_style));

    let in_out = format!(
        "In: {} / Out: {}",
        format_tokens(self.input_tokens),
        format_tokens(self.output_tokens)
    );
    spans.push(Span::styled(
        format!(" {} ", in_out),
        Style::default().fg(theme::TEXT_MUTED),
    ));

    if self.tps > 0.0 {
        spans.push(Span::styled(
            format!(" {:.0} t/s │", self.tps),
            Style::default().fg(theme::BORDER),
        ));
    }

    if self.context_size > 0 {
        let pct = if self.last_input_tokens > 0 {
            self.last_input_tokens * 100 / self.context_size
        } else {
            0
        };
        let pct_color = if pct >= 80 {
            theme::ERROR
        } else if pct >= 50 {
            theme::WARNING
        } else {
            theme::TEXT_MUTED
        };
        spans.push(Span::styled(
            format!("Ctx: {}% │", pct),
            Style::default().fg(pct_color),
        ));
    } else {
        spans.push(Span::styled("│".to_string(), Style::default().fg(theme::TEXT_MUTED)));
    }

    if let Some(ref id) = self.session_id {
        spans.push(Span::styled(
            format!(" Session: {} │ Calls: {} ", id, self.api_calls),
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }

    spans
}
```

- [ ] **Step 4: Update `render()` to draw one or two rows**

Replace the current `render()` body with this structure:

```rust
pub fn render(&self, area: Rect, buf: &mut Buffer) {
    if area.height == 0 {
        return;
    }

    let runtime_line = Line::from(self.runtime_row_spans());
    let width = area.width as usize;
    let lines = if area.height >= 2 {
        vec![
            runtime_line,
            Line::from(vec![Span::styled(
                self.context_row_text(width),
                Style::default().fg(theme::TEXT_MUTED),
            )]),
        ]
    } else {
        vec![runtime_line]
    };

    if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
        let (start, end) = if start < end { (start, end) } else { (end, start) };
        if start < end {
            let line = Line::from(self.highlighted_runtime_spans(start, end));
            let paragraph = Paragraph::new(line).style(Style::default().bg(theme::STATUS_BG));
            paragraph.render(area, buf);
            return;
        }
    }

    let paragraph = Paragraph::new(lines).style(Style::default().bg(theme::STATUS_BG));
    paragraph.render(area, buf);
}
```

Add helper used above:

```rust
fn highlighted_runtime_spans(&self, start: usize, end: usize) -> Vec<Span<'static>> {
    let full_text = self.runtime_row_text();
    let chars: Vec<char> = full_text.chars().collect();
    let len = chars.len();
    let sel_start = start.min(len);
    let sel_end = end.min(len);
    let before: String = safe_char_slice(&chars, 0, sel_start).iter().collect();
    let selected: String = safe_char_slice(&chars, sel_start, sel_end).iter().collect();
    let after: String = safe_char_slice(&chars, sel_end, len).iter().collect();
    let selection_style = Style::default().bg(theme::SELECTION_BG).fg(theme::SELECTION_FG);
    let base = Style::default().bg(theme::STATUS_BG);
    let mut highlighted = Vec::new();
    if !before.is_empty() {
        highlighted.push(Span::styled(before, base));
    }
    if !selected.is_empty() {
        highlighted.push(Span::styled(selected, selection_style));
    }
    if !after.is_empty() {
        highlighted.push(Span::styled(after, base));
    }
    highlighted
}
```

- [ ] **Step 5: Rename `build_full_text()` to runtime-specific helper**

Change:

```rust
fn build_full_text(&self) -> String {
```

to:

```rust
fn runtime_row_text(&self) -> String {
```

Update references in `get_selected_text()` and `screen_col_to_char_idx()` from `build_full_text()` to `runtime_row_text()`.

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test -p aemeath-cli status_bar -- --nocapture
```

Expected: all status bar tests PASS.

- [ ] **Step 7: Check file line count**

Run:

```bash
wc -l cli/src/tui/status_bar.rs
```

Expected: `cli/src/tui/status_bar.rs` is <= 400 lines. If it exceeds 400, move `WorktreeKind`, `StatusLineContext`, `shorten_path`, and width helpers into a new file `cli/src/tui/status_bar/context.rs` and add `mod context; use context::{...};` from `status_bar.rs` before committing.

- [ ] **Step 8: Commit**

Run:

```bash
git add cli/src/tui/status_bar.rs cli/src/tui/status_bar_tests.rs
git commit -m "feat(#46): render two-line status bar"
```

---

### Task 3: Allocate Two Rows in TUI Layout

**Files:**
- Modify: `cli/src/tui/app/mod.rs`

- [ ] **Step 1: Update layout height**

In `cli/src/tui/app/mod.rs`, replace:

```rust
Constraint::Length(1),
```

inside the main vertical layout constraints with:

```rust
Constraint::Length(2),
```

- [ ] **Step 2: Run focused status tests**

Run:

```bash
cargo test -p aemeath-cli status_bar -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run app build check**

Run:

```bash
cargo check -p aemeath-cli
```

Expected: PASS.

- [ ] **Step 4: Commit**

Run:

```bash
git add cli/src/tui/app/mod.rs
git commit -m "feat(#46): reserve two rows for status line"
```

---

### Task 4: Wire Initial Git Context and Documentation State

**Files:**
- Modify: `cli/src/tui/app/mod.rs`
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Add branch detection helper**

In `cli/src/tui/app/mod.rs`, add this helper near `display_working_dir`:

```rust
pub(crate) fn git_branch_for(path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("branch")
        .arg("--show-current")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}
```

- [ ] **Step 2: Add worktree detection helper**

In `cli/src/tui/app/mod.rs`, add this helper after `git_branch_for`:

```rust
pub(crate) fn worktree_kind_for(path: &Path) -> crate::tui::status_bar::WorktreeKind {
    let git_dir = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--git-dir")
        .output()
        .ok();
    let git_common = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("rev-parse")
        .arg("--git-common-dir")
        .output()
        .ok();

    let Some(git_dir) = git_dir else {
        return crate::tui::status_bar::WorktreeKind::Main;
    };
    let Some(git_common) = git_common else {
        return crate::tui::status_bar::WorktreeKind::Main;
    };
    if !git_dir.status.success() || !git_common.status.success() {
        return crate::tui::status_bar::WorktreeKind::Main;
    }

    let git_dir_text = String::from_utf8_lossy(&git_dir.stdout).trim().to_string();
    let git_common_text = String::from_utf8_lossy(&git_common.stdout).trim().to_string();
    if git_dir_text != git_common_text {
        crate::tui::status_bar::WorktreeKind::Worktree
    } else {
        crate::tui::status_bar::WorktreeKind::Main
    }
}
```

- [ ] **Step 3: Wire helpers in `App::new()`**

In `App::new()`, replace:

```rust
status_bar.set_current_dir(display_working_dir(&cwd));
```

with:

```rust
let cwd_display = cwd.display().to_string();
status_bar.set_context_paths(cwd_display.clone(), cwd_display);
if let Some(branch) = git_branch_for(&cwd) {
    status_bar.set_git_context(worktree_kind_for(&cwd), branch);
}
```

- [ ] **Step 4: Update feature doc to implementation state**

In `docs/feature/active.md`, replace the #46 table row status `规划完成，待实施` with `实现中` and detail section line:

```markdown
**状态**：规划完成，待实施
```

with:

```markdown
**状态**：实现中
```

- [ ] **Step 5: Run verification**

Run:

```bash
cargo test -p aemeath-cli status_bar -- --nocapture
cargo check -p aemeath-cli
```

Expected: both PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add cli/src/tui/app/mod.rs docs/feature/active.md
git commit -m "feat(#46): show initial git context in status line"
```

---

### Task 5: Final Verification, Mark #46待确认, Merge to Main

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Mark feature as pending confirmation**

In `docs/feature/active.md`, replace #46 status `实现中` with `待确认` in both table row and detail section.

- [ ] **Step 2: Run full verification**

Run:

```bash
cargo fmt --manifest-path cli/Cargo.toml
.agents/hooks/check-architecture-guards.sh
cargo test -p aemeath-cli status_bar -- --nocapture
cargo check -p aemeath-cli
```

Expected:
- `cargo fmt` exits 0.
- Architecture guard prints `All architecture guards passed.`
- Status bar tests PASS.
- `cargo check -p aemeath-cli` PASS.

- [ ] **Step 3: Commit final doc state**

Run:

```bash
git add docs/feature/active.md
git commit -m "docs(#46): mark status line feature pending confirmation"
```

If there are no doc changes because status was already updated in a previous commit, skip this commit and note it in the final response.

- [ ] **Step 4: Merge back to main**

Run from the main worktree:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
git merge --no-ff feature/46-status-line -m "Merge branch 'feature/46-status-line'"
```

Expected: merge succeeds.

- [ ] **Step 5: Verify on main**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath
.agents/hooks/check-architecture-guards.sh
cargo check -p aemeath-cli
git status --short
```

Expected:
- Architecture guard passes.
- `cargo check -p aemeath-cli` passes.
- `git status --short` is clean or only shows pre-existing unrelated changes.

- [ ] **Step 6: Clean worktree**

Run:

```bash
git worktree remove /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/feature-46-status-line
```

Expected: worktree removed.

---

## Self-Review

- Spec coverage: the plan covers two-line layout, balanced second-row fields, narrow truncation, worktree/branch display, permission mode display, tests, docs update, and main verification.
- Scope limitation: because current branch has no live `path_base`/`working_root` or permission engine source, the plan exposes setters and initializes from `App.cwd`; #43/#45/#42 can later feed the same `StatusLineContext` without renderer changes.
- No placeholders: all code snippets, commands, expected results, and file paths are explicit.
