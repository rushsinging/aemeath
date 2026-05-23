# Feature #46 Status Line V2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign the two-line TUI status line so line 1 shows runtime/model/token throughput/API call information and line 2 shows the actual working path, git/worktree identity, permission mode, and full session id.

**Architecture:** Keep the change local to the existing TUI status bar and app context wiring. `StatusBar` continues to own runtime counters and `StatusLineContext`, but formatting is changed to: line 1 = status/model/token-in/token-out/tokens-per-second/context percentage/API calls, line 2 = one primary path plus git/worktree plus permission plus full session id; only show both cwd/path_base and root/working_root when they differ. Path formatting must preserve a leading `~` or `/` on normal-width terminals so the user can verify it is a real path, not an abstract project label.

**Tech Stack:** Rust, ratatui `Line`/`Span` rendering, existing `theme` colors, `cargo test -p aemeath-cli status_bar -- --nocapture`, `.agents/hooks/check-architecture-guards.sh`, `cargo check -p aemeath-cli`.

---

## File Structure

- Modify `cli/src/tui/status_bar.rs`
  - Remove cost and session from visible runtime row; keep API calls.
  - Reorder runtime row to status, model, token in, token out, t/s, ctx%, API calls.
  - Render context row with semantic spans so the second row has visible colors.
- Modify `cli/src/tui/status_bar_format.rs`
  - Replace `ctx ... │ root ... │ ... │ Perm:...` default text with `path │ git │ permission │ session <full-id>`.
  - Preserve leading `~` or `/` where possible.
  - Include root only when `path_base != working_root` after normalization.
  - Show full session id in the context row when available.
- Modify `cli/src/tui/status_bar_tests.rs`
  - Update old expectations and add regression tests for no `ctx aemeath`, no cost, visible API calls, full session in context row, visible second-row colors, path prefix, root-only-when-different.
- Modify `cli/src/tui/app/mod.rs`
  - Stop using `display_working_dir()` for status line context; pass full cwd display path with home-directory compaction.
  - Keep `display_working_dir()` only if other callers still need a short label.
- Modify `docs/feature/active.md`
  - Update #46 planning text to the V2 accepted design and status.

## Current Code Facts

- `cli/src/tui/status_bar.rs` currently has `input_tokens`, `output_tokens`, `last_input_tokens`, `context_size`, and `tps` fields already wired.
- `App::draw()` sets tokens from `self.total_input_tokens`, `self.total_output_tokens`, and `self.last_input_tokens` before rendering.
- `UiEvent::TokensPerSecondUpdate` / `StreamingComplete` already call `status_bar.set_tps(tps)`.
- Current runtime row still contains `Think:ON/OFF`, verbose `Session`, and `Calls`; the user wants token in/out, t/s, and API calls on the runtime row, full session id on the context row, and does not want cost.
- Current context row starts with `ctx ...` and shows `root ...` even when both paths are the same, which the user rejected as confusing.
- Current `App::new()` calls `display_working_dir(&cwd)` and therefore collapses the path to `aemeath`; this must change to a real `~` or `/` path.

---

### Task 1: Redefine Runtime Row Text

**Files:**
- Modify: `cli/src/tui/status_bar.rs`
- Test: `cli/src/tui/status_bar_tests.rs`

- [ ] **Step 1: Add failing tests for runtime row content**

Append these tests to `cli/src/tui/status_bar_tests.rs`:

```rust
#[test]
fn test_runtime_row_shows_token_in_out_tps_ctx_and_api_without_cost_or_session() {
    let mut bar = StatusBar::new();
    bar.set_success("Ready");
    bar.set_model("zhipu/glm-5.1");
    bar.set_tokens(12_400, 1_800, 74_000);
    bar.set_context_size(200_000);
    bar.set_tps(42.0);
    bar.set_session_id("019-session");
    bar.set_api_calls(7);

    let text = bar.build_full_text();

    assert!(text.contains("Ready"));
    assert!(text.contains("zhipu/glm-5.1"));
    assert!(text.contains("in 12.4k"));
    assert!(text.contains("out 1.8k"));
    assert!(text.contains("42 t/s"));
    assert!(text.contains("ctx 37%"));
    assert!(text.contains("api 7"));
    assert!(!text.to_ascii_lowercase().contains("session"));
    assert!(!text.contains("019-session"));
    assert!(!text.to_ascii_lowercase().contains("cost"));
    assert!(!text.contains('$'));
}
```

- [ ] **Step 2: Run test and verify failure**

Run:

```bash
cd /Users/guoyuqi/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2
cargo test -p aemeath-cli test_runtime_row_shows_token_in_out_tps_ctx_and_api_without_cost_or_session -- --nocapture
```

Expected: FAIL because runtime row currently uses `In:` / `Out:` labels and still includes verbose `Session`; V2 runtime row should keep compact `api` but move session to the context row.

- [ ] **Step 3: Update runtime segments**

In `cli/src/tui/status_bar.rs`, replace the body of `fn runtime_segments(&self) -> Vec<(String, RuntimeSegmentStyle)>` with:

```rust
    fn runtime_segments(&self) -> Vec<(String, RuntimeSegmentStyle)> {
        let mut segments = Vec::new();
        segments.push((format!(" {} ", self.status), RuntimeSegmentStyle::Status(self.status_type)));

        if let Some(ref model) = self.model {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((format!(" {} ", model), RuntimeSegmentStyle::Model));
        }

        segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
        segments.push((
            format!(" in {} ", format_tokens(self.input_tokens)),
            RuntimeSegmentStyle::Muted,
        ));
        segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
        segments.push((
            format!(" out {} ", format_tokens(self.output_tokens)),
            RuntimeSegmentStyle::Muted,
        ));

        if self.tps > 0.0 {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((format!(" {:.0} t/s ", self.tps), RuntimeSegmentStyle::Muted));
        }

        if self.context_size > 0 {
            let pct = self.context_pct();
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((format!(" ctx {}% ", pct), RuntimeSegmentStyle::ContextPct(pct)));
        }

        if self.api_calls > 0 {
            segments.push(("│".to_string(), RuntimeSegmentStyle::Border));
            segments.push((format!(" api {} ", self.api_calls), RuntimeSegmentStyle::Muted));
        }

        segments
    }
```

Do not add cost or session id to `runtime_segments()`. Keep API calls visible in compact lowercase form.

- [ ] **Step 4: Run runtime row tests**

Run:

```bash
cargo test -p aemeath-cli test_runtime_row_shows_token_in_out_tps_ctx_and_api_without_cost_or_session -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit Task 1**

```bash
git add cli/src/tui/status_bar.rs cli/src/tui/status_bar_tests.rs
git commit -m "feat: simplify status line runtime row (refs #46)"
```

---

### Task 2: Redefine Context Row Formatting

**Files:**
- Modify: `cli/src/tui/status_bar_format.rs`
- Test: `cli/src/tui/status_bar_tests.rs`

- [ ] **Step 1: Add failing tests for path-first context row**

Append these tests to `cli/src/tui/status_bar_tests.rs`:

```rust
#[test]
fn test_context_row_uses_real_path_not_ctx_label_when_paths_match() {
    let mut bar = StatusBar::new();
    bar.set_context_paths(
        "~/Nextcloud/work/claudecode/aemeath",
        "~/Nextcloud/work/claudecode/aemeath",
    );
    bar.set_git_context(WorktreeKind::Main, "main");
    bar.set_permission_mode("AskMe");
    bar.set_session_id("019-session-full");

    let row = bar.context_row_text(100);

    assert_eq!(row, "~/Nextcloud/work/claudecode/aemeath │ main │ AskMe │ session 019-session-full");
    assert!(!row.contains("ctx "));
    assert!(!row.contains("root "));
    assert!(!row.contains("Perm:"));
}

#[test]
fn test_context_row_shows_root_only_when_different() {
    let mut bar = StatusBar::new();
    bar.set_context_paths(
        "~/Nextcloud/work/claudecode/aemeath/cli",
        "~/Nextcloud/work/claudecode/aemeath",
    );
    bar.set_git_context(WorktreeKind::Main, "main");
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text(120);

    assert!(row.contains("~/Nextcloud/work/claudecode/aemeath/cli"));
    assert!(row.contains("root ~/Nextcloud/work/claudecode/aemeath"));
    assert!(row.contains(" │ main │ AskMe"));
    assert!(row.contains("session 019-session-full"));
}

#[test]
fn test_context_row_worktree_uses_worktree_branch_label() {
    let mut bar = StatusBar::new();
    bar.set_context_paths(
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2",
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2",
    );
    bar.set_git_context(WorktreeKind::Worktree, "redesign/46-status-line-v2");
    bar.set_permission_mode("AskMe");

    let row = bar.context_row_text(120);

    assert!(row.contains("~"));
    assert!(row.contains(".worktrees/redesign-46-status-line-v2"));
    assert!(row.contains("worktree:redesign/46-status-line-v2"));
    assert!(row.ends_with("session 019-session-full"));
}

#[test]
fn test_context_row_narrow_preserves_path_git_and_permission() {
    let mut bar = StatusBar::new();
    bar.set_context_paths(
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2/cli/src/tui",
        "~/Nextcloud/work/claudecode/aemeath/.worktrees/redesign-46-status-line-v2",
    );
    bar.set_git_context(WorktreeKind::Worktree, "redesign/46-status-line-v2");
    bar.set_permission_mode("AllowAll");

    let row = bar.context_row_text(64);

    assert!(row.chars().count() <= 64);
    assert!(row.starts_with('~') || row.starts_with('…'));
    assert!(row.contains("worktree:") || row.contains("redesign/46-status-line-v2"));
    assert!(row.ends_with("session 019-session-full") || row.ends_with("AllowAll"));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p aemeath-cli context_row -- --nocapture
```

Expected: FAIL because current formatter emits `ctx`, `root`, and `Perm:` by default.

- [ ] **Step 3: Replace context row formatter**

In `cli/src/tui/status_bar_format.rs`, replace everything from `const FIELD_SEPARATOR` through `fn joined_len` with this implementation:

```rust
const FIELD_SEPARATOR: &str = " │ ";
const MIN_PATH_WIDTH: usize = 12;
const DEFAULT_PATH_WIDTH: usize = 54;
const DEFAULT_ROOT_WIDTH: usize = 36;

pub(crate) fn shorten_path(path: &str, max_chars: usize) -> String {
    if max_chars == 0 || path.is_empty() {
        return String::new();
    }
    let normalized = path.replace('\\', "/");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    if max_chars <= 2 {
        return "…".repeat(max_chars);
    }

    let prefix = if normalized.starts_with('~') {
        "~"
    } else if normalized.starts_with('/') {
        "/"
    } else {
        "…"
    };
    let prefix_len = prefix.chars().count();
    let tail_budget = max_chars.saturating_sub(prefix_len + 1).max(1);
    let tail: String = normalized
        .chars()
        .rev()
        .take(tail_budget)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}…{tail}")
}

pub(crate) fn truncate_to_char_count(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let suffix: String = text
        .chars()
        .rev()
        .take(max_chars - 1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{suffix}")
}

pub(crate) fn context_row_text(context: &StatusLineContext, width: usize) -> String {
    let fields = context_row_fields(context, width);
    let row = fields.join(FIELD_SEPARATOR);
    truncate_to_char_count(&row, width)
}

fn context_row_fields(context: &StatusLineContext, width: usize) -> Vec<String> {
    let git = git_text(context);
    let permission = context.permission_mode.clone();
    let session = context
        .session_id
        .as_ref()
        .filter(|session| !session.is_empty())
        .map(|session| format!("session {session}"));
    let paths_differ = normalized_path(&context.path_base) != normalized_path(&context.working_root);
    let fixed_len = git.chars().count()
        + permission.chars().count()
        + session.as_ref().map(|s| s.chars().count()).unwrap_or(0)
        + FIELD_SEPARATOR.chars().count()
            * ((if paths_differ { 3 } else { 2 }) + usize::from(session.is_some()));
    let available_for_paths = width.saturating_sub(fixed_len).max(MIN_PATH_WIDTH);

    let mut fields = Vec::new();
    if paths_differ {
        let path_width = available_for_paths.saturating_sub(DEFAULT_ROOT_WIDTH).max(MIN_PATH_WIDTH);
        fields.push(shorten_path(&context.path_base, path_width));
        fields.push(format!(
            "root {}",
            shorten_path(&context.working_root, DEFAULT_ROOT_WIDTH)
        ));
    } else {
        fields.push(shorten_path(&context.path_base, available_for_paths.min(DEFAULT_PATH_WIDTH)));
    }
    fields.push(git);
    fields.push(permission);
    if let Some(session) = session {
        fields.push(session);
    }
    fields
}

fn normalized_path(path: &str) -> String {
    path.trim_end_matches('/').replace('\\', "/")
}

fn git_text(context: &StatusLineContext) -> String {
    match (&context.worktree_kind, &context.branch) {
        (WorktreeKind::Main, Some(branch)) if branch == context.worktree_kind.label() => {
            context.worktree_kind.label().to_string()
        }
        (WorktreeKind::Main, Some(branch)) if !branch.is_empty() => branch.to_string(),
        (WorktreeKind::Main, _) => context.worktree_kind.label().to_string(),
        (WorktreeKind::Worktree, Some(branch)) if !branch.is_empty() => {
            format!("worktree:{branch}")
        }
        (WorktreeKind::Worktree, _) => context.worktree_kind.label().to_string(),
    }
}
```

- [ ] **Step 4: Update old context tests**

In `cli/src/tui/status_bar_tests.rs`, update or remove old tests that assert `ctx`, `root`, or `Perm:` as the default. Keep the semantic coverage by asserting:

```rust
assert!(!row.contains("ctx "));
assert!(row.ends_with("AskMe"));
```

For the existing `test_status_bar_selection_supports_context_row`, change the setup to:

```rust
bar.set_current_dir("~/aemeath");
```

and change the expected selection to:

```rust
assert_eq!(bar.get_selected_text(), Some("aemeath".to_string()));
```

with selection columns starting after the `~/` prefix.

- [ ] **Step 5: Run context row tests**

Run:

```bash
cargo test -p aemeath-cli context_row -- --nocapture
cargo test -p aemeath-cli status_bar_selection_supports_context_row -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

```bash
git add cli/src/tui/status_bar_format.rs cli/src/tui/status_bar_tests.rs
git commit -m "feat: redesign status line context row (refs #46)"
```

---

### Task 3: Add Colorful Context Row Spans

**Files:**
- Modify: `cli/src/tui/status_bar.rs`
- Test: `cli/src/tui/status_bar_tests.rs`

- [ ] **Step 1: Add failing render color test**

Append this test to `cli/src/tui/status_bar_tests.rs`:

```rust
#[test]
fn test_context_row_renders_path_git_and_permission_with_distinct_colors() {
    let mut bar = StatusBar::new();
    bar.set_context_paths("~/Nextcloud/work/claudecode/aemeath", "~/Nextcloud/work/claudecode/aemeath");
    bar.set_git_context(WorktreeKind::Main, "main");
    bar.set_permission_mode("AskMe");
    let area = Rect::new(0, 0, 100, 2);
    let mut buf = Buffer::empty(area);

    bar.render(area, &mut buf);

    assert_eq!(buf.cell((0, 1)).unwrap().style().fg, Some(theme::ACCENT));
    assert_eq!(buf.cell((45, 1)).unwrap().style().fg, Some(theme::SUCCESS));
    assert_eq!(buf.cell((52, 1)).unwrap().style().fg, Some(theme::WARNING));
    assert_eq!(buf.cell((0, 1)).unwrap().style().bg, Some(theme::STATUS_BG));
}
```

- [ ] **Step 2: Run test and verify failure**

Run:

```bash
cargo test -p aemeath-cli test_context_row_renders_path_git_and_permission_with_distinct_colors -- --nocapture
```

Expected: FAIL because the context row is currently one muted span.

- [ ] **Step 3: Add context row span builder**

In `cli/src/tui/status_bar.rs`, replace `fn context_row_spans(&self, width: usize, base: Style) -> Vec<Span<'static>>` with:

```rust
    fn context_row_spans(&self, width: usize, base: Style) -> Vec<Span<'static>> {
        let text = self.context_row_text(width);
        if self.selection_row == StatusBarRow::Context {
            return self.spans_with_selection(text, base);
        }
        let parts: Vec<&str> = text.split(" │ ").collect();
        let mut spans = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(" │ ", Style::default().fg(theme::BORDER)));
            }
            let style = match index {
                0 => Style::default().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
                1 if parts.len() == 4 => Style::default().fg(theme::TEXT_MUTED),
                index if index == parts.len().saturating_sub(2) => Style::default().fg(theme::SUCCESS),
                index if index == parts.len().saturating_sub(1) => Style::default().fg(theme::WARNING),
                _ => Style::default().fg(theme::TEXT_MUTED),
            };
            spans.push(Span::styled((*part).to_string(), style));
        }
        spans
    }
```

This keeps selection behavior simple: when the context row is selected, the selection overlay owns the row; when not selected, semantic colors are visible.

- [ ] **Step 4: Run render color tests**

Run:

```bash
cargo test -p aemeath-cli test_context_row_renders_path_git_and_permission_with_distinct_colors -- --nocapture
cargo test -p aemeath-cli test_status_bar_render_highlights_context_row_selection -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit Task 3**

```bash
git add cli/src/tui/status_bar.rs cli/src/tui/status_bar_tests.rs
git commit -m "feat: colorize status line context row (refs #46)"
```

---

### Task 4: Wire Full Path Display from App

**Files:**
- Modify: `cli/src/tui/app/mod.rs`
- Test: `cli/src/tui/status_bar_tests.rs`

- [ ] **Step 1: Add unit tests for home path compaction**

In `cli/src/tui/app/mod.rs`, add this helper near `display_working_dir`:

```rust
pub(crate) fn display_status_path(path: &Path) -> String {
    let raw = path.display().to_string();
    let Some(home) = dirs::home_dir() else {
        return raw;
    };
    let home = home.display().to_string();
    if raw == home {
        "~".to_string()
    } else if let Some(rest) = raw.strip_prefix(&(home + "/")) {
        format!("~/{rest}")
    } else {
        raw
    }
}
```

Append tests near the bottom of `cli/src/tui/app/mod.rs`:

```rust
#[cfg(test)]
mod status_path_tests {
    use super::*;

    #[test]
    fn test_display_status_path_returns_absolute_for_non_home_path() {
        let path = PathBuf::from("/tmp/aemeath-status-line");

        let display = display_status_path(&path);

        assert!(display.starts_with('/'));
        assert_eq!(display, "/tmp/aemeath-status-line");
    }

    #[test]
    fn test_display_working_dir_still_returns_leaf_name() {
        let path = PathBuf::from("/tmp/aemeath-status-line");

        let display = display_working_dir(&path);

        assert_eq!(display, "aemeath-status-line");
    }
}
```

- [ ] **Step 2: Update App::new status context wiring**

In `cli/src/tui/app/mod.rs`, replace:

```rust
let cwd_display = display_working_dir(&cwd);
status_bar.set_context_paths(cwd_display.clone(), cwd_display);
```

with:

```rust
let cwd_display = display_status_path(&cwd);
status_bar.set_context_paths(cwd_display.clone(), cwd_display);
```

- [ ] **Step 3: Run app path tests**

Run:

```bash
cargo test -p aemeath-cli status_path_tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Commit Task 4**

```bash
git add cli/src/tui/app/mod.rs
git commit -m "feat: show real path in status line context (refs #46)"
```

---

### Task 5: Update Feature Tracking and Final Verification

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Update #46 document text**

In `docs/feature/active.md`, update the #46 table row target and detail section to state:

```markdown
status line V2：第一行展示运行状态、模型、token in/out、t/s、ctx%、session、api calls；不显示 cost；第二行默认展示一个真实工作路径（`~` 或 `/` 开头）、git/worktree、权限模式；仅当 cwd/path_base 与 working_root 不一致时显示 `root ...`。
```

In the acceptance criteria, include these bullets:

```markdown
- 第一行必须包含 token in、token out、tokens/s、ctx%、api calls，且不显示 cost 或 session。
- 第二行不得默认显示 `ctx aemeath`；路径必须可识别为 `~` 或 `/` 开头的真实路径，窄屏可中间省略。
- 第二行在 session 存在时必须显示完整 session id（格式 `session <id>`）。
- cwd/path_base 与 working_root 相同时只显示一个路径；不一致时才显示 `root ...`。
- git 主分支显示 `main`，worktree 显示 `worktree:<branch>`，不得出现 `main:main`。
- 权限模式直接显示 `AskMe` / `AllowAll` 等值，不加空的 `Perm:` 标签。
- 第二行非选中状态必须有语义颜色；选中状态必须有可见选区背景并保持可复制文本。
```

- [ ] **Step 2: Run full status line tests**

Run:

```bash
cargo test -p aemeath-cli status_bar -- --nocapture
cargo test -p aemeath-cli status_path_tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run architecture and compile verification**

Run:

```bash
.agents/hooks/check-architecture-guards.sh
cargo check -p aemeath-cli
```

Expected: both PASS.

- [ ] **Step 4: Check file sizes**

Run:

```bash
wc -l cli/src/tui/status_bar.rs cli/src/tui/status_bar_format.rs cli/src/tui/status_bar_selection.rs cli/src/tui/status_bar_tests.rs cli/src/tui/app/mod.rs
```

Expected: every `.rs` file is under 400 lines. If `status_bar_tests.rs` exceeds 400, split newly added V2 tests into `cli/src/tui/status_bar_v2_tests.rs`, add it with `#[cfg(test)] #[path = "status_bar_v2_tests.rs"] mod v2_tests;` from `status_bar.rs`, and rerun the checks.

- [ ] **Step 5: Commit Task 5**

```bash
git add docs/feature/active.md
git commit -m "docs: update status line v2 tracking (refs #46)"
```

---

## Self-Review

- Spec coverage: runtime row, token in/out, t/s, ctx%, API calls, removal of cost/runtime-session, full session in context row, path-first context row, cwd/root deduplication, worktree label, permission display, color regression, and docs tracking are each covered by tasks.
- Placeholder scan: no TBD/TODO/fill-in-later placeholders remain.
- Type consistency: plan uses existing `StatusBar`, `StatusLineContext`, `WorktreeKind`, `set_context_paths`, `set_git_context`, `set_permission_mode`, `set_tokens`, `set_tps`, and `context_row_text` names already present in the current codebase.
