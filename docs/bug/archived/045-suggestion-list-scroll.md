# Bug #45: / 命令自动补全时上下键不能翻页选择候选

**状态**：已修复并确认（2026-05-18）
**优先级**：中
**发现日期**：2026-05

## 症状

输入 `/` 后弹出命令候选列表，按上下键无法在超过可见区域（5 条）的候选项中移动高亮。选中项超出前 5 条时高亮消失，视觉上看不到当前选中了哪一项。

## 根因

`render_suggestions_in_area()` 始终从第 0 条开始渲染（`self.suggestions.iter().take(max_visible)`），没有 scroll offset。当 `selected_suggestion >= max_visible` 时，选中项不在渲染范围内，高亮不可见。

## 修复方案

渲染时计算 `scroll_offset`：当 `selected_suggestion >= max_visible` 时，从 `selected - max_visible + 1` 处开始渲染，确保选中项始终在可见区域内。

## 验证

- `cargo test -p aemeath-cli` 通过（136 passed）
- 手动验证：输入 `/` 后上下键可在所有候选项间移动，列表自动滚动跟随

## 修复 Commit

- `5f98e2e` — fix(tui): 建议列表渲染添加 scroll offset 跟随选中位置 refs #45

## 涉及路径

- `aemeath-cli/src/tui/input_area/suggestions.rs`（`render_suggestions_in_area()`）
