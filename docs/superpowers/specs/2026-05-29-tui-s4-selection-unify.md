# #59 S4：input/status 选区迁入 view_state + mouse_handler 选区统一

**日期**：2026-05-29
**所属**：feature #59 子项 **S4**
**前置**：S2（output 选区入 view_state + adapter 范式）、S3（StatusBar runtime 去镜像，但**选区未迁**）、#56（input text/cursor 入 model，但**选区未迁**）已合并

## 问题（调研裁定，纠正 roadmap 隐含前提）
S2/S3/#56 **未**达成"三区选区统一"：
- **output**：S2 已单源（真相在 `view_state::OutputViewState`，mouse_handler 改 view_state，`adapter/output_view_widget.rs` 每帧写回 widget 镜像）。✅
- **input**：选区真相**仍在 widget**（`render/input/input_area/`），`model.input.document.selection` 是从未赋值的死桩（#56 只迁 text/cursor）。
- **status**：选区真相**仍在 widget**（`render/status/bar.rs` + `display/status_bar_selection.rs`），S3 明确不碰。

mouse_handler 仍直驱 input_area/status_bar 的 start/update/end/clear_selection，以及跨区 clear（含 output_area.clear_selection）。

## 设计决策：对齐 S2，迁 view_state（不引入 Model 选区 intent）
S2 已确立范式：**选区真相在 view_state**；mouse_handler 作为 **effectful 边界**直接 mutate view_state（非纯 reducer/非 Model intent，`handle_mouse_event` 已返回 `Vec<Effect>`、已在 `App::update` 路径）；adapter 每帧写回 widget 镜像。S4 **对齐此范式补齐 input/status**，**NEVER** 为选区引入 Model 层 intent（会与 S2 分裂、过度设计）。"统一"= 三区都走 view_state 单源，不是引入新抽象层。

不搞统一选区 adapter 抽象：三区坐标系差异大（output `(line,CharIdx)`、input textarea `(row,col)`、status row 枚举+char_idx+width），沿 S2 风格各加 `apply_*_selection_to_widget` 函数即可。

## 实现
### view_state（新增）
- `InputSelectionViewState`：textarea `(row,col)` 锚点 + is_selecting；begin/update/end/clear/normalize。
- `StatusSelectionViewState`：status 选区（row 枚举/标识 + char_idx + width，按现 `status_bar_selection.rs` 模型）；begin/update/end/clear。
- 各带三路径单测（normalize/CJK/边界）。坐标→锚点折算（依赖 render 期 textarea/line_text/status 布局）保留在 widget 只读借用（对齐 output 的 `screen_to_anchor` 留 widget）。

### adapter（新增，仿 output_view_widget.rs）
- `apply_input_selection_to_widget(view, input_area)`、`apply_status_selection_to_widget(view, status_bar)`：view_state→widget 选区镜像单向写回。每帧渲染前调（input 接 `input_widget.rs` 或新文件；status 接 `status_widget.rs` 或新文件）。

### mouse_handler 全分支改 view_state
- input 区 Down/Drag/Up → `view_state.input_sel.*`；status 区 → `view_state.status_sel.*`；跨区 clear 统一改 view_state（清另两区的 view_state 选区）。
- Up 取文本仿 output：先 `apply_*_selection_to_widget` 同步镜像，再 `widget.get_selected_text()`，再 clear view_state（消一帧滞后）。复制仍走现有 `copy_selection_to_clipboard` → Effect。

### resize/reset
`app/resize.rs`、`app/runtime.rs::reset_runtime_state` 已清 output 选区；补清 input/status 选区 view_state。

### 死桩清理
删除 `model/input/document.rs` 的未接线 `selection: Option<InputSelection>` 字段 + `InputSelection` 类型（grep 确认从未赋非 None）。

### Guard
新建 `.agents/hooks/check-tui-selection-single-source.sh`：禁业务路径直驱 `input_area.{start_selection,update_selection,end_selection}`、`status_bar.{start_selection_at,update_selection_at,end_selection}`、三区 `clear_selection`，禁直写选区字段；豁免对应 adapter + widget 内部 reset + 测试。同步收紧现有 `check-tui-output-scroll-selection-single-source.sh` 对 `input_area/`、`display/` 目录的豁免（迁移后这些目录选区字段应只由 adapter 写）。注册进 `check-architecture-guards.sh`。

## 非目标
- 不引入 Model 层选区 intent（对齐 S2 view_state 范式）。
- 不重写复制时序/Effect（沿用 #56/S5 已有）。
- 不动 output 选区（S2 已完成）。
- 不动 input text/cursor（#56）、status runtime 镜像（S3）。

## 迁移分步（每步独立编译 + `cargo test -p cli` 通过；先 status 后 input——status 坐标简单风险低）
1. **T1**：`StatusSelectionViewState` + `apply_status_selection_to_widget` adapter + 单测（不接线 mouse）。
2. **T2**：mouse_handler status 分支 + 跨区清 status 改 view_state（原子：adapter 接入渲染前管线 + mouse 改写同步）。
3. **T3**：`InputSelectionViewState` + `apply_input_selection_to_widget` adapter + 单测；删 `document.selection` 死桩。
4. **T4**：mouse_handler input 分支 + 跨区清 input 改 view_state（原子）。
5. **T5**：resize/reset 补清 input/status 选区 view_state；删 widget 选区状态方法（grep 确认无生产调用后）。
6. **T6**：guard + 收紧 output guard 豁免；全量 `cargo test -p cli` + clippy + 所有 hook 通过。

## 测试
- view_state input/status 选区：begin/update/end/clear/normalize、CJK、边界。
- adapter：view_state→widget 镜像。
- mouse_handler：三区选区改写后行为不变（高亮/复制）。
- resize/reset 清三区选区（仿 S2 回归测试）。
- 复制内容不变（plain）。
