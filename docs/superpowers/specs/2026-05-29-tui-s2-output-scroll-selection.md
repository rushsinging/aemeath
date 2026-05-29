# #59 S2：OutputArea 滚动/follow-tail/选区迁入 view_state 单源

**日期**：2026-05-29
**所属**：feature #59 子项 **S2**
**前置**：roadmap §4 S2；#63（gutter/选区列偏移）、S1（spinner/task 入 Model + 派生范式）已合并

## 问题
`OutputArea` widget 自持滚动态（`scroll_offset`/`auto_scroll`）、follow-tail、输出区选区态（`is_selecting`/`selection_start`/`selection_end`），由 `app/update/key_scroll.rs`、`render/input/mouse_handler.rs` 命令式直改。违反单源。

## 设计决策：放 view_state（非 conversation/runtime Model）
滚动偏移与选区锚点是**纯易变 UI 交互态**——无会话语义、无需持久化、与对话业务无关。类比 S1（spinner 业务态 active/phase 入 Model，frame/verb 动画细节入 view_state），滚动/选区属"动画细节"侧 → 入 **`view_state`**。

`view_state/output.rs::OutputViewState` 已存在（含 scroll_offset/auto_scroll/follow_tail/selection 字段，但大部分为死脚手架，且选区类型 `SelectedTextRange{block_key}` 与 widget 实际坐标 `(usize 逻辑行, CharIdx)` 不符）。**复用并对齐**：选区字段类型改为 widget 现用的 `(usize, CharIdx)`（或 `OutputSelectionAnchor{line:usize, col:CharIdx}`），删除不匹配的 `SelectedTextRange`/`ScreenLineMapEntry` 草稿与未接线的 `adapter/mouse_event.rs` 死脚手架。

## 保留（#63 衔接，不变）
- 选区坐标系 = `(逻辑行, plain CharIdx)`。
- gutter_cols 列补偿（屏幕列 → 减 gutter_cols → plain char）。
- 高亮经 `apply_selection_overlay`（plain 列区间）；复制读 plain。
- widget 渲染逻辑（`visible_range`/`sel_range_for_line`/scrollbar/overlay）不动，仅改为读"由 adapter 从 view_state 写回的镜像"。
- `last_visible_height`/`screen_line_map` 仍由 render 回填，反喂 view_state 供下次 scroll 钳制。

## S2 vs S4 边界
- **S2**：滚动/选区**状态**入 `OutputViewState`；新增其纯变更方法；新增 adapter `apply_output_view_to_widget` 每帧从 view_state 单向写回 widget；`key_scroll.rs` + `mouse_handler.rs`（滚轮 + output 选区分支）改为改 view_state。
- **S4（不在本范围）**：mouse_handler 三 widget（output/input/status）统一 + 全程 Intent/effect 派发重构。S2 允许 mouse_handler 仍直接 mutate view_state（不强行升格为 conversation Intent——选区是 UI 态，无需进 reducer），保持改面最小。

## 实现
### view_state/output.rs
对齐 `OutputViewState`：`scroll_offset: usize`、`auto_scroll: bool`、`is_selecting: bool`、`selection_start/end: Option<(usize, CharIdx)>`、`last_visible_height: usize`（+ 反喂的 screen_line_map 若需）。删 `SelectedTextRange`/`ScreenLineMapEntry`/`version`(恒 0 死字段)/`follow_tail`(与 auto_scroll 重复则合一)。
新增纯变更方法（搬 `render/output_area/scroll.rs` + `selection.rs` 逻辑）：`scroll_up(amount, total_lines, visible)`、`scroll_down(amount)`、`scroll_to_bottom()`、`scroll_to_top(total_lines)`、`begin_selection`/`update_selection`/`end_selection`/`clear_selection`/`select_word`（保留 gutter_cols 补偿 + `(行,CharIdx)` 坐标 + plain 列折算）。每方法单测（搬现有断言）。

### adapter/output_view_widget.rs（新增，仿 adapter/live_status_widget.rs）
`apply_output_view_to_widget(view_state: &OutputViewState, output_area: &mut OutputArea)`：单向写回 widget 的 `scroll_offset/auto_scroll/selection_start/selection_end/is_selecting` 镜像。每帧渲染前（`app/update.rs` 的 refresh 管线，`refresh_output_widget_from_model` 之后）调用。

### 交互接线
- `app/update/key_scroll.rs`：PageUp/Down/Shift+Up/Down/Home/End → 改 `app.view_state.output.scroll_*`（不再直调 widget）。
- `render/input/mouse_handler.rs`：滚轮 → view_state scroll；output 选区分支（start/update/end/select_word/clear）→ view_state 选区方法。input/status 的 clear_selection 暂留（S4 统一）。复制仍走现有 Effect。

### Guard
新增 `.agents/hooks/check-tui-output-scroll-selection-single-source.sh`（仿 spinner-task guard）：禁 `(output|output_area|self)\.(scroll_offset|auto_scroll|is_selecting|selection_start|selection_end)\s*=` 直写、禁直调 `output_area.(scroll_up|scroll_down|scroll_to_bottom|start_selection|update_selection|end_selection|select_word|clear_selection)\(`，豁免新 adapter + `content.rs`(reset) + `*_tests.rs`。注册进 `check-architecture-guards.sh`。

## 非目标
- 不动 mouse_handler 三 widget 统一 / Intent 派发（S4）。
- 不动 input/status 选区（input 属 #56 已完成；status 属 S3 已完成）。
- 不动 #63 渲染/gutter/overlay 逻辑（仅迁状态宿主）。

## 迁移分步（每步独立编译 + `cargo test -p cli` 通过）
1. 对齐 `OutputViewState` 字段类型（`(usize,CharIdx)`），加 scroll 方法 + 单测（搬 scroll.rs 逻辑）。
2. 加选区方法 + 单测（搬 selection.rs，含 gutter_cols/select_word/get_selected_text）。
3. 新增 `apply_output_view_to_widget` adapter + 测试，接入 update.rs 渲染前管线。
4. `key_scroll.rs` + mouse_handler 滚轮 → view_state；行为不变。
5. mouse_handler output 选区分支 → view_state；迁 resize/state/selection 测试到 view_state（或保留 widget 作 adapter 写回后镜像验证）。
6. 删 widget 上 scroll.rs/selection.rs 公开方法（降为 adapter 私有/删）；删死脚手架（mouse_event.rs、SelectedTextRange 等）。
7. 加 guard + 接入；全量 `cargo test -p cli` + clippy + 所有 hook 通过。

## 测试
- view_state scroll：up/down/to_bottom/to_top、auto_scroll 切换、钳制边界。
- view_state 选区：begin/update/end/clear/select_word、gutter_cols 补偿、CJK、get_selected_text plain 切片、跨行。
- adapter：view_state → widget 镜像写回正确。
- 回归：#63 选区列偏移测试、scroll 钳制、follow-tail 行为不变。
