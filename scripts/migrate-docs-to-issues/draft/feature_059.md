<!-- Migrated from: docs/feature/archived/059-tui-single-source-roadmap.md -->
# Feature #59：TUI Model/View 单源迁移收口（伞型 roadmap）

**状态**：✅ 已完成（2026-05-30 用户确认，S1-S5 全部完成）

**优先级**：中

## 背景

对账 #53/#55/#56/#57/#58 已有迁移成果与 6 个现存 guard，列出真正未被守护的剩余单源缺口，定义收口顺序。采用单写入者/单向 + guard 范式（非"状态零留 widget"）。详见 [roadmap](../superpowers/specs/2026-05-29-tui-single-source-completion-roadmap.md)。

## 子项

### S1 OutputArea live tail（spinner + task window 入 Model）✅

`SpinnerModel{active, phase}` + `SpinnerPhase` 入 RuntimeModel；动画 frame/verb 归 view_state；`LiveStatusViewModel` + assembler + adapter 单向派生；30 处触发点转 `RuntimeIntent`；task lines 经 `UpdateTaskLines` 入 Model；`check-tui-spinner-task-single-source.sh` guard。spec：`2026-05-29-tui-s1-spinner-task-single-source.md`。

### S2 OutputArea 滚动/follow-tail/选区入 Model ✅

`scroll_offset`/`auto_scroll`/选区锚点入 `view_state::OutputViewState`；`adapter/output_view_widget.rs` 每帧单向写回 widget 镜像；`key_scroll`/滚轮/鼠标选区改 view_state；保留 #63 `gutter_cols` 列补偿 + plain 复制；删 widget `scroll.rs`/死选区方法/`mouse_event.rs` 脚手架；`check-tui-output-scroll-selection-single-source.sh` guard。spec：`2026-05-29-tui-s2-output-scroll-selection.md`。

### S3 StatusBar 去镜像 + 单写入者 ✅

StatusBar 状态去镜像，单写入者范式落地。

### S4 选区统一 + mouse_handler 走 intent ✅

input/status 选区迁入 view_state（`InputSelectionViewState`/`StatusSelectionViewState`，对齐 S2 output 范式）；三区选区真相均在 view_state，adapter 每帧单向写回 widget 镜像；mouse_handler 三区+跨区清全走 view_state（零 widget 直驱选区）；widget 屏幕坐标→锚点折算保留为只读；删 `model.input.document.selection` 死桩 + widget 选区状态方法；resize/reset 清三区；`check-tui-selection-single-source.sh` guard + 收紧 output guard 豁免。spec：`2026-05-29-tui-s4-selection-unify.md`。

### S5 Effect 化 tea-purity 豁免名单内副作用 ✅

- **A 块**：reflection/clipboard（前期）+ dialog/suggestions `list_models` 改预取缓存（`cached_models`）去 `block_on`、`/save` 复用 `Effect::SaveSession{notify}`、`/memory` 新增 `Effect::FetchMemoryList`、`/paste` 复用 `Effect::ReadClipboardImage`，相关文件移出 tea-purity EXEMPT。
- **B 块 wontfix**：`slash.rs` 主分发为 request-response + `Option<String>` 控制流，Effect 化需整管线状态机重写收益低成本高，连同 `mod.rs`/`run_loop`/`runtime`/`slash_tests` 标合理豁免并在 guard 注释文档化。

spec：`2026-05-29-tui-s5-gap-slash-effect.md`。

## 验证

`cargo test -p cli`、`.agents/hooks/check-architecture-guards.sh` 通过。2026-05-30 用户确认 feature #59 已完成。
