# TUI Safe Text Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a TUI-only safe text/indexing layer and migrate high-risk selection/render/input paths to it so string/index/screen-map panics stop recurring.

**Architecture:** Add `aemeath-cli/src/tui/safe_text.rs` as the only place that performs char-range clamping, char-based string slicing, Unicode-width truncation, display-column conversion, and dynamic split-index clamping. Existing TUI modules call this API instead of writing direct `chars[from..to]`, `s[i..j]`, or unchecked `split_off(offset)` logic. A grep-based guard script prevents unsafe patterns from returning to TUI business code.

**Tech Stack:** Rust, ratatui, tui-textarea, unicode-width, cargo test, shell script guard.

---

## File Structure

- Create: `aemeath-cli/src/tui/safe_text.rs`
  - Owns panic-free TUI text/index helpers.
  - Contains all unit tests for safe text helpers.
- Modify: `aemeath-cli/src/tui/mod.rs`
  - Exposes `safe_text` module.
- Modify: `aemeath-cli/src/tui/output_area/selection.rs`
  - Uses `safe_char_slice()` and `safe_str_slice_by_char()`.
- Modify: `aemeath-cli/src/tui/output_area/mod.rs`
  - Uses `clamp_split_index()` before `screen_line_map.split_off()`.
- Modify: `aemeath-cli/src/tui/output_area/display.rs`
  - Delegates `truncate_unicode_width()` and `screen_col_to_char_idx()` to `safe_text`.
- Modify: `aemeath-cli/src/tui/input_area.rs`
  - Uses `safe_char_slice()` for auto-wrap suffix extraction.
- Create: `scripts/check-unsafe-text-ops.sh`
  - Fails on unsafe TUI text/index patterns outside `safe_text.rs`.
- Modify: `docs/feature/active.md`
  - Update #23 status and implementation notes.

---

### Task 1: Add `safe_text` module with tests

**Files:**
- Create: `aemeath-cli/src/tui/safe_text.rs`
- Modify: `aemeath-cli/src/tui/mod.rs`

- [ ] **Step 1: Create `safe_text.rs` with tests and implementation**

Create `aemeath-cli/src/tui/safe_text.rs` with this content:

```rust
use std::ops::Range;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn clamp_char_range(from: usize, to: usize, chars_len: usize) -> Option<Range<usize>> {
    let from = from.min(chars_len);
    let to = to.min(chars_len);
    if from >= to {
        None
    } else {
        Some(from..to)
    }
}

pub fn safe_char_slice(chars: &[char], from: usize, to: usize) -> &[char] {
    match clamp_char_range(from, to, chars.len()) {
        Some(range) => &chars[range],
        None => &[],
    }
}

pub fn safe_str_slice_by_char(s: &str, from: usize, to: usize) -> &str {
    let char_len = s.chars().count();
    let Some(range) = clamp_char_range(from, to, char_len) else {
        return "";
    };
    let byte_start = char_to_byte_clamped(s, range.start);
    let byte_end = char_to_byte_clamped(s, range.end);
    &s[byte_start..byte_end]
}

pub fn safe_char_at(s: &str, idx: usize) -> Option<char> {
    s.chars().nth(idx)
}

pub fn truncate_unicode_width(s: &str, max_cols: usize) -> (&str, usize) {
    if max_cols == 0 {
        return ("", 0);
    }
    if s.width() <= max_cols {
        return (s, s.width());
    }

    let mut width = 0usize;
    let mut end = 0usize;
    for (byte_idx, ch) in s.char_indices() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width > max_cols {
            break;
        }
        width += ch_width;
        end = byte_idx + ch.len_utf8();
    }
    (&s[..end], width)
}

pub fn col_to_char_idx(s: &str, col: usize) -> usize {
    let mut width = 0usize;
    for (char_idx, ch) in s.chars().enumerate() {
        let ch_width = ch.width().unwrap_or(1);
        if width + ch_width > col {
            return char_idx;
        }
        width += ch_width;
    }
    s.chars().count()
}

pub fn clamp_split_index(offset: usize, len: usize) -> usize {
    offset.min(len)
}

fn char_to_byte_clamped(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_char_range_normal() {
        assert_eq!(clamp_char_range(1, 3, 5), Some(1..3));
    }

    #[test]
    fn test_clamp_char_range_empty_or_reversed() {
        assert_eq!(clamp_char_range(2, 2, 5), None);
        assert_eq!(clamp_char_range(4, 2, 5), None);
    }

    #[test]
    fn test_clamp_char_range_out_of_bounds() {
        assert_eq!(clamp_char_range(1, 99, 4), Some(1..4));
        assert_eq!(clamp_char_range(99, 100, 4), None);
    }

    #[test]
    fn test_safe_char_slice_ascii() {
        let chars: Vec<char> = "hello".chars().collect();
        assert_eq!(safe_char_slice(&chars, 1, 4), &['e', 'l', 'l']);
    }

    #[test]
    fn test_safe_char_slice_cjk_and_emoji() {
        let chars: Vec<char> = "你🚀好".chars().collect();
        assert_eq!(safe_char_slice(&chars, 0, 2), &['你', '🚀']);
        assert_eq!(safe_char_slice(&chars, 2, 99), &['好']);
    }

    #[test]
    fn test_safe_char_slice_invalid_range_returns_empty() {
        let chars: Vec<char> = "abc".chars().collect();
        assert!(safe_char_slice(&chars, 3, 3).is_empty());
        assert!(safe_char_slice(&chars, 9, 10).is_empty());
        assert!(safe_char_slice(&chars, 2, 1).is_empty());
    }

    #[test]
    fn test_safe_str_slice_by_char_ascii() {
        assert_eq!(safe_str_slice_by_char("hello", 1, 4), "ell");
    }

    #[test]
    fn test_safe_str_slice_by_char_utf8_boundaries() {
        assert_eq!(safe_str_slice_by_char("你🚀好", 0, 2), "你🚀");
        assert_eq!(safe_str_slice_by_char("你🚀好", 2, 99), "好");
    }

    #[test]
    fn test_safe_str_slice_by_char_invalid_range_returns_empty() {
        assert_eq!(safe_str_slice_by_char("abc", 2, 1), "");
        assert_eq!(safe_str_slice_by_char("abc", 9, 10), "");
    }

    #[test]
    fn test_safe_char_at_bounds() {
        assert_eq!(safe_char_at("你a", 0), Some('你'));
        assert_eq!(safe_char_at("你a", 1), Some('a'));
        assert_eq!(safe_char_at("你a", 2), None);
    }

    #[test]
    fn test_truncate_unicode_width_ascii() {
        assert_eq!(truncate_unicode_width("hello", 3), ("hel", 3));
        assert_eq!(truncate_unicode_width("hi", 3), ("hi", 2));
    }

    #[test]
    fn test_truncate_unicode_width_cjk() {
        assert_eq!(truncate_unicode_width("你好世界", 4), ("你好", 4));
        assert_eq!(truncate_unicode_width("你好", 1), ("", 0));
    }

    #[test]
    fn test_truncate_unicode_width_emoji() {
        assert_eq!(truncate_unicode_width("a🚀b", 3), ("a🚀", 3));
        assert_eq!(truncate_unicode_width("a🚀b", 2), ("a", 1));
    }

    #[test]
    fn test_col_to_char_idx_ascii_cjk_emoji() {
        assert_eq!(col_to_char_idx("hello", 2), 2);
        assert_eq!(col_to_char_idx("你好", 2), 1);
        assert_eq!(col_to_char_idx("a🚀b", 2), 1);
        assert_eq!(col_to_char_idx("a🚀b", 99), 3);
    }

    #[test]
    fn test_clamp_split_index() {
        assert_eq!(clamp_split_index(0, 3), 0);
        assert_eq!(clamp_split_index(2, 3), 2);
        assert_eq!(clamp_split_index(9, 3), 3);
    }
}
```

- [ ] **Step 2: Expose the module**

Edit `aemeath-cli/src/tui/mod.rs` and add `safe_text` between `output_area` and `status_bar`:

```rust
pub mod app;
pub mod completion;
pub mod dialog;
pub mod input_area;
pub mod output_area;
pub mod safe_text;
pub mod status_bar;

pub use app::App;
pub use input_area::InputArea;
pub use output_area::OutputArea;
pub use status_bar::StatusBar;
```

- [ ] **Step 3: Run tests**

Run:

```bash
cargo test -p aemeath-cli safe_text -- --nocapture
```

Expected: all `safe_text` tests pass.

- [ ] **Step 4: Commit**

```bash
git add aemeath-cli/src/tui/safe_text.rs aemeath-cli/src/tui/mod.rs
git commit -m "feat(tui): 新增安全文本索引工具"
```

---

### Task 2: Migrate Bug #28 selection/render paths

**Files:**
- Modify: `aemeath-cli/src/tui/output_area/selection.rs`
- Modify: `aemeath-cli/src/tui/output_area/mod.rs`

- [ ] **Step 1: Update selection imports**

In `aemeath-cli/src/tui/output_area/selection.rs`, add this import after the existing `aemeath_core` import:

```rust
use crate::tui::safe_text::{safe_char_slice, safe_str_slice_by_char};
```

- [ ] **Step 2: Replace debug preview slicing**

In `end_selection()`, replace the current `selected.as_deref().map(|s| { ... })` block with:

```rust
selected
    .as_deref()
    .map(|s| safe_str_slice_by_char(s, 0, 100))
```

- [ ] **Step 3: Replace selected text slicing**

In `get_selected_text()`, replace the manual `from > to` guard and `result.extend(chars[from..to].iter());` with:

```rust
let selected_chars = safe_char_slice(&chars, from, to);
if selected_chars.is_empty() {
    log::debug!(
        "get_selected_text: empty clamped range logic={}, from={}, to={}, chars_len={}",
        logic_idx,
        from,
        to,
        chars.len()
    );
    continue;
}
log::debug!(
    "get_selected_text: logic={}, from={}, to={}, chars_len={}, content={:?}",
    logic_idx,
    from,
    to,
    chars.len(),
    safe_str_slice_by_char(&self.lines[logic_idx].content, 0, 60)
);
result.extend(selected_chars.iter());
```

- [ ] **Step 4: Update render split clamping**

In `aemeath-cli/src/tui/output_area/mod.rs`, add this import near other crate imports:

```rust
use crate::tui::safe_text::clamp_split_index;
```

Find the render trimming block that computes `mapped_drop = offset.min(self.screen_line_map.len());` and replace that line with:

```rust
let mapped_drop = clamp_split_index(offset, self.screen_line_map.len());
```

- [ ] **Step 5: Run regression tests**

Run:

```bash
cargo test -p aemeath-cli test_get_selected_text -- --nocapture
cargo test -p aemeath-cli test_render_clamps_screen_line_map_when_reserved_lines_overflow_height -- --nocapture
```

Expected:
- `test_get_selected_text_clamps_start_col_after_line_shrinks` passes.
- `test_get_selected_text_skips_line_when_clamped_start_exceeds_end` passes.
- `test_render_clamps_screen_line_map_when_reserved_lines_overflow_height` passes.

- [ ] **Step 6: Commit**

```bash
git add aemeath-cli/src/tui/output_area/selection.rs aemeath-cli/src/tui/output_area/mod.rs
git commit -m "fix(tui): 使用安全文本 API 修复输出区越界"
```

---

### Task 3: Delegate display width helpers to `safe_text`

**Files:**
- Modify: `aemeath-cli/src/tui/output_area/display.rs`

- [ ] **Step 1: Update imports**

Replace the top imports in `aemeath-cli/src/tui/output_area/display.rs`:

```rust
use aemeath_core::string_idx::CharIdx;
use unicode_width::UnicodeWidthChar;
```

with:

```rust
use aemeath_core::string_idx::CharIdx;
use crate::tui::safe_text;
```

- [ ] **Step 2: Delegate `truncate_unicode_width`**

Replace the body of `pub fn truncate_unicode_width(s: &str, max_width: usize) -> String` with:

```rust
pub fn truncate_unicode_width(s: &str, max_width: usize) -> String {
    let (prefix, width_used) = safe_text::truncate_unicode_width(s, max_width);
    if width_used == s.width() {
        return s.to_string();
    }
    if max_width <= 3 {
        return "...".chars().take(max_width).collect();
    }
    let target = max_width - 3;
    let (prefix, _) = safe_text::truncate_unicode_width(s, target);
    format!("{prefix}...")
}
```

Then add this import because the wrapper still uses `.width()`:

```rust
use unicode_width::UnicodeWidthStr;
```

- [ ] **Step 3: Delegate `screen_col_to_char_idx`**

Replace the body of `screen_col_to_char_idx()` with:

```rust
pub fn screen_col_to_char_idx(text: &str, screen_col: usize) -> CharIdx {
    CharIdx::new(safe_text::col_to_char_idx(text, screen_col))
}
```

- [ ] **Step 4: Run display tests**

Run:

```bash
cargo test -p aemeath-cli display -- --nocapture
cargo test -p aemeath-cli test_get_selected_text -- --nocapture
```

Expected: display-related tests and selection tests pass.

- [ ] **Step 5: Commit**

```bash
git add aemeath-cli/src/tui/output_area/display.rs
git commit -m "refactor(tui): 复用安全文本宽度转换"
```

---

### Task 4: Migrate input auto-wrap suffix slicing

**Files:**
- Modify: `aemeath-cli/src/tui/input_area.rs`

- [ ] **Step 1: Update imports**

Add this import near the top of `aemeath-cli/src/tui/input_area.rs`:

```rust
use crate::tui::safe_text::safe_char_slice;
```

- [ ] **Step 2: Replace suffix extraction**

In `auto_wrap_current_line()`, replace:

```rust
let after: String = chars[best_break..].iter().collect();
```

with:

```rust
let after: String = safe_char_slice(&chars, best_break, chars.len())
    .iter()
    .collect();
```

- [ ] **Step 3: Add input auto-wrap regression tests**

At the end of `aemeath-cli/src/tui/input_area.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn test_auto_wrap_current_line_handles_cjk_without_panic() {
        let mut input = InputArea::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 12,
            height: 3,
        };
        let mut buf = Buffer::empty(area);
        input.render(area, &mut buf);

        for ch in "你好世界你好世界".chars() {
            input.input(ch);
        }

        assert!(input.get_text().contains('你'));
    }

    #[test]
    fn test_auto_wrap_current_line_handles_emoji_without_panic() {
        let mut input = InputArea::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 12,
            height: 3,
        };
        let mut buf = Buffer::empty(area);
        input.render(area, &mut buf);

        for ch in "a🚀b🚀c🚀d🚀e".chars() {
            input.input(ch);
        }

        assert!(input.get_text().contains('🚀'));
    }
}
```

- [ ] **Step 4: Run input tests**

Run:

```bash
cargo test -p aemeath-cli test_auto_wrap_current_line -- --nocapture
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add aemeath-cli/src/tui/input_area.rs
git commit -m "fix(tui): 安全处理输入区自动换行切片"
```

---

### Task 5: Add unsafe text operation guard script

**Files:**
- Create: `scripts/check-unsafe-text-ops.sh`

- [ ] **Step 1: Create guard script**

Create `scripts/check-unsafe-text-ops.sh` with this content:

```bash
#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/aemeath-cli/src/tui"
FAILED=0

while IFS= read -r -d '' file; do
  rel="${file#$ROOT/}"
  case "$rel" in
    aemeath-cli/src/tui/safe_text.rs)
      continue
      ;;
  esac

  while IFS=: read -r line_no line; do
    if [[ "$line" == *"allow unsafe_text_op"* ]]; then
      continue
    fi
    printf 'unsafe text op: %s:%s:%s\n' "$rel" "$line_no" "$line"
    FAILED=1
  done < <(
    perl -ne '
      print "$.:$_" if /\.chars\(\)\.nth\(/;
      print "$.:$_" if /chars\s*\[[^\]]*\.\.[^\]]*\]/;
      print "$.:$_" if /\.split_off\s*\(/;
      print "$.:$_" if /&\s*[A-Za-z_][A-Za-z0-9_]*\s*\[[^\]]*\.\.[^\]]*\]/;
    ' "$file"
  )
done < <(find "$TARGET" -name '*.rs' -print0)

if [[ "$FAILED" -ne 0 ]]; then
  echo "Unsafe TUI text/index operations found. Use crate::tui::safe_text helpers or add an explicit allow unsafe_text_op comment for ASCII-only cases."
  exit 1
fi
```

- [ ] **Step 2: Make it executable**

Run:

```bash
chmod +x scripts/check-unsafe-text-ops.sh
```

- [ ] **Step 3: Run the guard**

Run:

```bash
scripts/check-unsafe-text-ops.sh
```

Expected: if it reports existing violations, handle them in Task 6. Do not commit the script as passing until Task 6 is complete.

---

### Task 6: Resolve guard findings in TUI code

**Files:**
- Modify as reported by `scripts/check-unsafe-text-ops.sh`

- [ ] **Step 1: Run guard and capture findings**

Run:

```bash
scripts/check-unsafe-text-ops.sh
```

Expected: output lists exact file and line for unsafe operations, or no output.

- [ ] **Step 2: Fix common findings**

Use these replacements:

1. For `chars[from..to]`:

```rust
let slice = crate::tui::safe_text::safe_char_slice(&chars, from, to);
```

2. For `s.chars().nth(idx)`:

```rust
let ch = crate::tui::safe_text::safe_char_at(s, idx);
```

3. For `vec.split_off(offset)` with dynamic offset:

```rust
let offset = crate::tui::safe_text::clamp_split_index(offset, vec.len());
let tail = vec.split_off(offset);
```

4. For ASCII-only protocol marker slicing, add a same-line comment:

```rust
let marker = &text[..MARKER.len()]; // allow unsafe_text_op: MARKER is ASCII and boundary checked
```

- [ ] **Step 3: Re-run guard**

Run:

```bash
scripts/check-unsafe-text-ops.sh
```

Expected: no output and exit 0.

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p aemeath-cli safe_text -- --nocapture
cargo test -p aemeath-cli test_get_selected_text -- --nocapture
cargo test -p aemeath-cli test_render_clamps_screen_line_map_when_reserved_lines_overflow_height -- --nocapture
cargo test -p aemeath-cli test_auto_wrap_current_line -- --nocapture
```

Expected: all listed tests pass.

- [ ] **Step 5: Commit**

```bash
git add scripts/check-unsafe-text-ops.sh aemeath-cli/src/tui
git commit -m "test(tui): 增加不安全文本操作门禁"
```

---

### Task 7: Update feature tracking and final verification

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Update Feature #23 status**

In `docs/feature/active.md`, change row #23 status from `待实施` to `待确认` and append this implementation note under `### #23 TUI 字符串/切片安全索引收口`:

```markdown
**已完成的改动**：
1. 新增 `aemeath-cli/src/tui/safe_text.rs`，统一提供 panic-free 字符范围、字符串切片、显示宽度截断、列号转换、split index clamp。
2. `selection.rs` 的复制选中文本路径迁移到 `safe_char_slice` / `safe_str_slice_by_char`。
3. `output_area/mod.rs` 的 `screen_line_map.split_off` 迁移到 `clamp_split_index`。
4. `output_area/display.rs` 的宽度截断和列号转换委托给 `safe_text`。
5. `input_area.rs` 自动换行后缀提取改为 `safe_char_slice`。
6. 新增 `scripts/check-unsafe-text-ops.sh` 门禁，阻止 TUI 业务路径重新出现高风险切片/索引写法。
```

- [ ] **Step 2: Run full verification**

Run:

```bash
cargo fmt
scripts/check-unsafe-text-ops.sh
cargo test -p aemeath-cli safe_text -- --nocapture
cargo test -p aemeath-cli test_get_selected_text -- --nocapture
cargo test -p aemeath-cli test_render_clamps_screen_line_map_when_reserved_lines_overflow_height -- --nocapture
cargo test -p aemeath-cli test_auto_wrap_current_line -- --nocapture
cargo check -p aemeath-cli
git diff --check
```

Expected:
- guard exits 0
- all listed tests pass
- `cargo check -p aemeath-cli` exits 0
- `git diff --check` exits 0

- [ ] **Step 3: Commit docs and final adjustments**

```bash
git add docs/feature/active.md
git commit -m "docs(feature): 标记 TUI 安全文本索引待确认"
```

---

## Self-Review Checklist

- Spec coverage:
  - `safe_text.rs` API covered by Task 1.
  - #28 migration covered by Task 2.
  - display/input high-risk migration covered by Tasks 3-4.
  - grep guard covered by Tasks 5-6.
  - docs update and verification covered by Task 7.
- Placeholder scan:
  - No TBD/TODO placeholders.
  - Each command and expected result is explicit.
- Type consistency:
  - Function names match the design: `clamp_char_range`, `safe_char_slice`, `safe_str_slice_by_char`, `safe_char_at`, `truncate_unicode_width`, `col_to_char_idx`, `clamp_split_index`.
  - Module path is consistently `crate::tui::safe_text`.
