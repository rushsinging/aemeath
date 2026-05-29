# #59 S2 实现计划：OutputArea 滚动/选区入 view_state 单源

> 用 superpowers:subagent-driven-development 逐 Task 执行。每步独立编译 + `cargo test -p cli` 通过。
> Spec：`docs/superpowers/specs/2026-05-29-tui-s2-output-scroll-selection.md`（务必先读：状态放 view_state；保留 #63 选区坐标系/gutter_cols/plain overlay；S2 不碰 mouse_handler 三 widget 统一[S4]）。

**Goal:** 把 OutputArea 自持的 scroll_offset/auto_scroll/选区态迁入 `view_state::OutputViewState`，新增 adapter 单向写回 widget 镜像，key_scroll/mouse_handler 改 view_state，加 guard。

**验证门禁（每 Task）:** `cargo test -p cli`、`cargo clippy -p cli`、`bash .agents/hooks/check-architecture-guards.sh`（共享 target 缓存被其他 worktree 污染时先 `cargo clean -p <crate>`）。所有改动在 worktree `feature/59-s2-output-scroll-sel`。

参考样板：`adapter/live_status_widget.rs`（S1 adapter 范式）、`adapter/output_widget.rs`、`.agents/hooks/check-tui-spinner-task-single-source.sh`。

---

## Task 1：OutputViewState scroll 方法（对齐字段 + 搬 scroll 逻辑）
**Files:** `view_state/output.rs`（对齐字段类型；删 SelectedTextRange/ScreenLineMapEntry/version 死字段；加 scroll 方法 + 单测）。
- 字段保留/对齐：`scroll_offset: usize`、`auto_scroll: bool`、`is_selecting: bool`、`selection_start/end: Option<(usize, CharIdx)>`、`last_visible_height: usize`。
- 方法（搬 `render/output_area/scroll.rs` 逻辑）：`scroll_up(amount, total_lines)`（用 last_visible_height 算 max_offset 钳制、auto_scroll=false、max_offset==0 时复位）、`scroll_down(amount)`（归零置 auto_scroll=true）、`scroll_to_bottom()`、`scroll_to_top(total_lines)`。
- 单测：scroll up/down 钳制、auto_scroll 切换、to_bottom/to_top、边界（max_offset==0）。
- **不接线**（widget 旧方法仍在）。commit `feat(tui): OutputViewState scroll 方法 (refs #59 S2)`。

## Task 2：OutputViewState 选区方法（搬 selection 逻辑 + 保留 gutter_cols）
**Files:** `view_state/output.rs`（加选区方法 + 单测）。
- 方法（搬 `render/output_area/selection.rs` 逻辑）：`begin_selection(row,col,...)`、`update_selection`、`end_selection`、`clear_selection`、`select_word`，保留 gutter_cols 列补偿（屏幕列→减 gutter_cols→plain char，经 `display::screen_col_to_char_idx`）与 `(逻辑行, CharIdx)` 坐标。`get_selected_text`/`sel_range_for_line` 逻辑可留 widget 渲染侧读镜像（选区状态在 view_state，渲染映射仍在 render.rs）——明确哪些搬、哪些留：**选区锚点状态**(start/end/is_selecting) 入 view_state；**屏幕坐标→锚点的换算**（依赖 screen_line_map/gutter_cols，render 期数据）可保留在一个接受这些数据的方法里。实现者据现状定最小切分，报告。
- 单测：begin/update/end/clear/select_word、gutter_cols 补偿、CJK、跨行。
- commit `feat(tui): OutputViewState 选区方法 (refs #59 S2)`。

## Task 3：adapter 写回 + 接线渲染管线
**Files:** Create `adapter/output_view_widget.rs`；Modify `app/update.rs`（refresh 管线）、`adapter/mod.rs`。
- `apply_output_view_to_widget(view_state: &OutputViewState, output_area: &mut OutputArea)`：写回 widget 的 scroll_offset/auto_scroll/selection_start/end/is_selecting 镜像。
- 在 `app/update.rs` 渲染前管线（`refresh_output_widget_from_model` 之后、`apply_live_status_to_widget` 附近）调用。
- 反喂：render 回填的 `last_visible_height` 同步回 view_state（供 scroll 钳制）——实现者确定回填点。
- 单测：view_state→widget 镜像。commit `feat(tui): output 滚动/选区 adapter 写回 + 接线 (refs #59 S2)`。

## Task 4：key_scroll + 滚轮 → view_state
**Files:** `app/update/key_scroll.rs`、`render/input/mouse_handler.rs`（滚轮分支）。
- PageUp/Down、Shift+Up/Down/Home/End → `app.view_state.output.scroll_*`（传 total_lines = `output_area.document.total_lines()` 或等价）。
- 滚轮 ScrollUp/Down → view_state scroll。
- 行为不变（滚动表现一致）。commit `refactor(tui): 滚动键/滚轮改 view_state (refs #59 S2)`。

## Task 5：mouse_handler output 选区分支 → view_state
**Files:** `render/input/mouse_handler.rs`（output 选区分支）、迁移 resize/state/selection 测试。
- Down(output 区)→view_state begin/select_word；Drag→update_selection；Up→end_selection（复制仍走现有 Effect）。input/status 的 clear_selection 暂留（S4）。
- 迁 `app/resize.rs`/`app/state/tests.rs`/selection 测试到 view_state（或保留 widget 作 adapter 写回后镜像验证，不弱化）。
- 行为不变。commit `refactor(tui): output 选区交互改 view_state (refs #59 S2)`。

## Task 6：删 widget 公开方法 + 死脚手架
**Files:** `render/output_area/scroll.rs`、`selection.rs`、`adapter/mouse_event.rs`、`view_state/output.rs`。
- widget 上 scroll/selection 公开方法降级为 adapter 私有镜像或删除（grep 确认无非 adapter/测试调用）。
- 删未接线死脚手架：`adapter/mouse_event.rs`（未被生产调用）、`OutputViewState` 残留草稿类型。
- commit `refactor(tui): 删 widget 滚动/选区公开方法与死脚手架 (refs #59 S2)`。

## Task 7：guard
**Files:** Create `.agents/hooks/check-tui-output-scroll-selection-single-source.sh`；Modify `check-architecture-guards.sh`。
- 禁 `(output|output_area|self)\.(scroll_offset|auto_scroll|is_selecting|selection_start|selection_end)\s*=` 直写 + 禁直调 widget scroll/selection 方法；豁免新 adapter + content.rs(reset) + `*_tests.rs`。
- 验证 clean=0 / 注入违规=1。注册。全量 test+clippy+hook 通过。commit `feat(tui): output 滚动/选区单源 guard (refs #59 S2)`。

## Self-Review
- 状态在 view_state、widget 仅镜像（adapter 单向写）✓
- #63 选区坐标系/gutter_cols/plain overlay 保留 ✓
- S2 不碰 mouse_handler 三 widget 统一（S4）✓
- guard 验证非空 ✓
