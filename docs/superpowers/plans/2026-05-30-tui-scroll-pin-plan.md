# TUI 滚动位置固定（Scroll Pin）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用户向上滚动查看历史内容时，滚动位置保持固定，不受底部新增内容影响。

**Architecture:** 在 `OutputViewState` 中记录上一帧文档行数，渲染前检测文档增长，当 `auto_scroll=false` 时将增长量补偿到 `scroll_offset`，保持视窗顶部行号不变。

**Tech Stack:** Rust, ratatui

---

### Task 1: OutputViewState 新增 `last_document_total_lines` 字段

**Files:**
- Modify: `apps/cli/src/tui/view_state/output.rs`

- [ ] **Step 1: 在 struct 和 Default 中添加字段**

在 `OutputViewState` struct 中新增 `pub last_document_total_lines: usize`，默认值为 `0`。

**位置1：struct 定义（第11-18行），在 `last_visible_height` 后面加一行：**

```rust
pub last_document_total_lines: usize,
```

**位置2：`Default` 实现（第32行 `last_visible_height: 0` 后）：**

```rust
last_document_total_lines: 0,
```

**注意**：`OutputViewState` derive 了 `Clone, Debug, Eq, PartialEq`，新增字段为 `usize`，与 derive 相容，无需额外处理。

- [ ] **Step 2: 更新现有测试中的构造以兼容新字段**

现有测试中有多处显式构造 `OutputViewState`（如第167-171行、第184-189行等），但它们都用 `..Default::default()` 展开，会自动填充 `last_document_total_lines: 0`，因此无需修改测试。确认一次即可。

- [ ] **Step 3: 添加新字段默认值测试**

在 `tests` 模块末尾（`test_last_document_total_lines_default_zero` 之前没有这个测试），添加：

```rust
#[test]
fn test_last_document_total_lines_default_zero() {
    let state = OutputViewState::default();
    assert_eq!(state.last_document_total_lines, 0);
}
```

- [ ] **Step 4: 编译验证**

Run: `cargo build -p aemeath-cli 2>&1 | tail -5`
Expected: 编译成功，无错误。

- [ ] **Step 5: 运行测试**

Run: `cargo test -p aemeath-cli -- view_state::output 2>&1 | tail -10`
Expected: 所有测试通过（包括新增的 `test_last_document_total_lines_default_zero`）。

- [ ] **Step 6: 提交**

```bash
git add apps/cli/src/tui/view_state/output.rs
git commit -m "feat(tui): OutputViewState 新增 last_document_total_lines 字段"
```

---

### Task 2: 内容增长补偿与测试

**Files:**
- Modify: `apps/cli/src/tui/adapter/output_view_widget.rs`
- Modify: `apps/cli/src/tui/view_state/output.rs` (tests)

- [ ] **Step 1: 在 view_state/output.rs 测试中新增滚动补偿测试**

在 `view_state/output.rs` 的 `tests` 模块末尾（第353行 `}` 之前）新增三个测试：

```rust
#[test]
fn test_scroll_pin_growth_compensates_offset() {
    let mut state = OutputViewState {
        last_visible_height: 10,
        scroll_offset: 5,
        auto_scroll: false,
        last_document_total_lines: 30,
        ..Default::default()
    };
    // 内容从 30 行增长到 40 行（Δ=10），scroll_offset 应增加 10。
    // 但 max_offset = 40 - 10 = 30，5+10=15 < 30，不触发钳制。
    // 此测试验证"补偿"概念无误：增长量加到 offset。
    let growth = 40usize.saturating_sub(state.last_document_total_lines);
    assert!(!state.auto_scroll);
    assert_eq!(growth, 10);
    let expected = state.scroll_offset.saturating_add(growth);
    assert_eq!(expected, 15);
}

#[test]
fn test_scroll_pin_shrink_no_compensation() {
    let mut state = OutputViewState {
        last_visible_height: 10,
        scroll_offset: 12,
        auto_scroll: false,
        last_document_total_lines: 30,
        ..Default::default()
    };
    // 内容从 30 行收缩到 20 行，growth=0（saturating_sub），不应补偿。
    let new_total = 20usize;
    let growth = new_total.saturating_sub(state.last_document_total_lines);
    assert_eq!(growth, 0);
    // offset(12) 超出 max_offset(20-10=10)，钳制后应为 10。
    let max_offset = new_total.saturating_sub(state.last_visible_height);
    assert_eq!(max_offset, 10);
    let clamped = state.scroll_offset.min(max_offset);
    assert_eq!(clamped, 10);
}

#[test]
fn test_scroll_pin_auto_scroll_true_skips_compensation() {
    let state = OutputViewState {
        last_visible_height: 10,
        scroll_offset: 0,
        auto_scroll: true,
        last_document_total_lines: 30,
        ..Default::default()
    };
    // auto_scroll=true 时不触发补偿。
    assert!(state.auto_scroll);
    let growth = 40usize.saturating_sub(state.last_document_total_lines);
    assert_eq!(growth, 10);
    // 但不加补偿。
    let compensated = if !state.auto_scroll {
        state.scroll_offset.saturating_add(growth)
    } else {
        state.scroll_offset
    };
    assert_eq!(compensated, 0);
}
```

- [ ] **Step 2: 运行测试确认新测试通过**

Run: `cargo test -p aemeath-cli -- view_state::output 2>&1 | tail -15`
Expected: 全部测试通过。

- [ ] **Step 3: 在 output_view_widget.rs 中实现增长补偿逻辑**

修改 `apply_output_scroll_to_widget` 函数（第15-35行）。**替换整个函数体**，在第①步"反喂可见高度"和第②步"钳制"之间插入补偿逻辑。

完整替换后的函数：

```rust
/// 据 view_state 滚动真相写回 widget 镜像（含 last_visible_height 反喂 + 内容增长补偿 + 钳制）。
///
/// 时序（每帧渲染前）：
/// 1. 把上一帧 render 回填的可见高度反喂回 view_state；
/// 2. 检测文档行数增长，auto_scroll=false 时补偿 scroll_offset，保持视窗内容固定；
/// 3. 钳制 scroll_offset 到 max_offset；
/// 4. 把钳制后的 view_state 滚动态写回 widget 镜像。
pub(crate) fn apply_output_scroll_to_widget(
    view: &mut OutputViewState,
    output_area: &mut OutputArea,
) {
    // ① 反喂上一帧渲染回填的可见高度。
    view.last_visible_height = output_area.last_visible_height;

    // ② 内容增长补偿：auto_scroll=false 时保持视窗顶部行号不变。
    let new_total = output_area.document().total_lines();
    if !view.auto_scroll {
        let growth = new_total.saturating_sub(view.last_document_total_lines);
        view.scroll_offset = view.scroll_offset.saturating_add(growth);
    }
    view.last_document_total_lines = new_total;

    // ③ 钳制 stale offset（迁自旧 clamp_scroll_state，真相归 view_state）。
    let max_offset = new_total.saturating_sub(view.last_visible_height);
    view.scroll_offset = view.scroll_offset.min(max_offset);
    if view.scroll_offset == 0 {
        view.auto_scroll = true;
    }

    // ④ 单向写回 widget 镜像。
    output_area.scroll_offset = view.scroll_offset;
    output_area.auto_scroll = view.auto_scroll;
}
```

同时更新文件头注释（第1-9行），替换为：

```rust
//! 输出区滚动 adapter：把 `OutputViewState` 的滚动真相单向写回 `OutputArea`
//! 的 `scroll_offset` / `auto_scroll` 镜像。这是这两个镜像字段的唯一生产写入路径。
//!
//! 时序（每帧渲染前）：
//! 1. 把上一帧 render 回填的 `output_area.last_visible_height` 反喂回 view_state，
//!    供滚动钳制使用（view_state 不持有 document/可见高度，由 render 期回填）；
//! 2. 检测文档行数增长量，`auto_scroll=false` 时补偿 `view_state.scroll_offset`，
//!    保持用户视窗内容固定（不受底部新增内容影响）；
//! 3. 据 document 总行数与可见高度钳制 view_state.scroll_offset（迁自旧
//!    `output_widget.rs::clamp_scroll_state`，真相归 view_state）；
//! 4. 把钳制后的 view_state 滚动态写回 widget 镜像。
```

- [ ] **Step 4: 在 output_view_widget.rs tests 中新增补偿测试**

在 `output_view_widget.rs` 的 `tests` 模块末尾（第147行 `}` 之前）新增两个测试：

```rust
#[test]
fn test_apply_compensates_for_content_growth_when_not_auto_scroll() {
    let mut view = OutputViewState {
        scroll_offset: 5,
        auto_scroll: false,
        last_document_total_lines: 50,
        ..Default::default()
    };
    let mut output = OutputArea::new();
    output.last_visible_height = 20;
    // 内容 60 行（比上一帧多 10 行）
    output.set_plain_document_lines(60);

    apply_output_scroll_to_widget(&mut view, &mut output);

    // 正常路径：内容增长 10 行，offset 应从 5 补偿到 15（保持视窗内容固定）。
    // max_offset = 60 - 20 = 40，15 < 40 不触发钳制，auto_scroll 保持 false。
    assert_eq!(view.scroll_offset, 15);
    assert!(!view.auto_scroll);
    assert_eq!(view.last_document_total_lines, 60);
    assert_eq!(output.scroll_offset, 15);
    assert!(!output.auto_scroll);
}

#[test]
fn test_apply_no_compensation_when_auto_scroll() {
    let mut view = OutputViewState {
        scroll_offset: 0,
        auto_scroll: true,
        last_document_total_lines: 50,
        ..Default::default()
    };
    let mut output = OutputArea::new();
    output.last_visible_height = 20;
    // 内容从 50 → 70（增长 20 行），但 auto_scroll=true 不补偿
    output.set_plain_document_lines(70);

    apply_output_scroll_to_widget(&mut view, &mut output);

    // auto_scroll=true：scroll_offset 保持 0（贴尾），不受增长影响。
    assert_eq!(view.scroll_offset, 0);
    assert!(view.auto_scroll);
    assert_eq!(view.last_document_total_lines, 70);
    assert_eq!(output.scroll_offset, 0);
    assert!(output.auto_scroll);
}
```

- [ ] **Step 5: 编译验证**

Run: `cargo build -p aemeath-cli 2>&1 | tail -5`
Expected: 编译成功。

- [ ] **Step 6: 运行测试**

Run: `cargo test -p aemeath-cli -- output_view_widget 2>&1 | tail -15`
Expected: 所有测试通过（包括新增的两个测试）。
Run: `cargo test -p aemeath-cli -- view_state::output 2>&1 | tail -15`
Expected: 所有测试通过。

- [ ] **Step 7: 运行完整 clippy 检查**

Run: `cargo clippy -p aemeath-cli 2>&1 | tail -10`
Expected: 无新的 warning。

- [ ] **Step 8: 提交**

```bash
git add apps/cli/src/tui/adapter/output_view_widget.rs apps/cli/src/tui/view_state/output.rs
git commit -m "feat(tui): 滚动位置固定 — 内容增长时补偿 scroll_offset"
```

---

### 验证清单

实现完成后，确认以下行为：

1. **贴底模式不变**：auto_scroll=true 时，新内容正常追加，视窗自动跟随底部。
2. **滚动后固定**：用户向上滚动（auto_scroll=false）后，新内容在下方生成，视窗不移动。
3. **用户滚回底部**：scroll_offset 归零时 auto_scroll 自动恢复为 true。
4. **内容收缩**：不影响正确性。
