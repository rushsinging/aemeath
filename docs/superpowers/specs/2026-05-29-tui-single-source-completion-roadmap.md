# TUI Model/View 单源迁移收口 Roadmap（Feature #85）

> 状态：roadmap（决策文档，非单一实现 spec）。每个子项 S1–S5 落地时各自走独立 spec → plan → 实施。

## 1. 背景

TUI 自 feature #53 起进行 Model/View（TEA/Elm）单源迁移，后续由 #55（render/adapter/app 三层收口）、#56（输入单一真相）、#57（目录物理收口）、#58（输出渲染管线统一，进行中）推进。

本 roadmap 不另起炉灶，而是**对账已有迁移成果与现存 guard，列出真正尚未被守护的剩余单源缺口**，并定义收口顺序。

## 2. 采用的"单源"诠释

项目采用 **单写入者 / 单向数据流（single-writer, unidirectional）**，而非"状态零留 widget"的严格诠释：

- 状态**可以**物理留在 widget 上，但**唯一**的修改路径必须是 `*Intent → *Model::apply → *Change → adapter 写回 widget`。
- 任何 `app/update` 业务逻辑**不得**把 widget 字段当业务真相读取。
- 每条单源约束由一个 `.agents/hooks/check-tui-*.sh` guard 在架构守卫中焊死，防回归。

#56 的 `check-tui-input-single-source.sh` 即此模式样板（已覆盖 input_area 的 text/cursor/suggestions/history mutation）。

## 3. 现存 guard 覆盖（已守护，不在本 roadmap 范围）

| Guard | 覆盖 |
|---|---|
| `check-tui-effect-boundary.sh` | `model/`、`update/` 禁止 spawn/Command/hook/clipboard/block_on/`.await`/mpsc |
| `check-tui-tea-purity.sh` | `app/`、`model/`、`view_assembler/`、`view_model/` 禁止副作用（含 EXEMPT 名单） |
| `check-tui-model-view-boundaries.sh` | model 纯净、view_model 不依赖 model 内部、view_assembler 无 ratatui/副作用、SDK 事件先经 adapter |
| `check-tui-input-single-source.sh` | input_area mutation 仅经 adapter（#56） |
| `check-tui-output-legacy-guards.sh` | 禁止 legacy 渲染兜底 |
| `check-tui-toplevel-layout.sh` | TUI 顶层 9 层目录白名单 |

## 4. 真正剩余缺口（未被任何 guard 守护）

### S1 — OutputArea live tail：spinner + task window 入 Model
**违规**：`render/output_area/mod.rs` 的 `spinner: Option<SpinnerState>`、`task_status_lines: Vec<String>` 为 widget 自持状态，经 ~30 处 `start_spinner/set_spinner_phase/stop_spinner`（散落 `update/ui_event.rs`、`enter.rs`、`done.rs`、`ask_user_key.rs`、`slash/reflection.rs`、`run_loop.rs`）命令式修改。
**目标**：
- 状态进 `RuntimeModel`：新增 `SpinnerModel{active, verb, phase, started_at, frame}`；扩展 `TaskStatusSnapshot` 携带逐条 `TaskItemView{title, display_number, state}`（现仅有计数）。
- 触发收敛为 `SpinnerIntent{Start, Stop, SetPhase, Tick}`；phase 文案归拢为语义枚举 `SpinnerPhase`（消除 5 文件重复字面量）。
- 渲染：新增 `LiveStatusViewModel{spinner, task_window}` + `LiveStatusAssembler`（复用既有纯函数 `build_task_window`）；spinner 不进 BlockCache（动画特性），每帧重组。
- 渲染合成：`output_area/render.rs::append_status_lines` 改读 ViewModel。
**新 guard**：禁止 `output_area` 上直接 spinner/task mutation。
**依赖**：无。最小、自包含，建议首做。

### S2 — OutputArea 滚动 / follow-tail / 选区入 Model
**违规**：`output_area/mod.rs` 的 `scroll_offset`、`auto_scroll`、`is_selecting`、`selection_start/end` 为 widget 权威状态（Model 无对应）；`update/key_scroll.rs` 滚动键直调 `output_area.scroll_*`；`render.rs` 在渲染中回写缓存（render 不纯）。
**目标**：滚动/follow-tail/输出选区进 Model（新 `conversation.view` 或独立 view 子模型）；滚动/选区改 intent；`OutputViewModel.follow_tail_hint` 升为权威而非 hint。
**新 guard**：禁止外部直接改 output_area scroll/selection。
**依赖**：无（但选区部分与 S4 协同）。

### S3 — StatusBar 去镜像 + 单写入者
**违规**：`status/bar.rs` 自持 `input_tokens/output_tokens/tps/model/session_id/context` —— 全是 `RuntimeModel`/`SessionModel` 镜像；`adapter/status_widget.rs` 从 Model 拷入，`update/ui_event.rs` 又直接 `status_bar.set_tps/set_tokens/set_git_context` —— **双写入者，分歧风险**。`StatusBar::render` 读自身字段而非 `StatusViewModel`（assembler 已存在但闲置）。
**目标**：StatusBar 仅由 `StatusViewModel`（经 `view_assembler/status.rs`）单向派生；删除 `ui_event.rs` 直写；StatusBar 选区同 S4。
**新 guard**：禁止 `status_bar.set_*` 出现在 update 业务路径。
**依赖**：无。

### S4 — 选区统一 + mouse_handler 走 intent
**违规**：`render/input/mouse_handler.rs` 一个函数直驱 output_area / input_area / status_bar 三者的 `start/update/end/clear_selection`、`select_word`、`scroll_*` —— 完全绕过 Model。
**目标**：三处选区状态先入各自 Model（S1/S2/S3 中完成），mouse_handler 改为产出选区/滚动 intent，由 adapter 写回。
**依赖**：S2（输出选区）、S3（状态栏选区）、#56（输入选区已入 model）。**排在 S2/S3 之后。**

### S5 — Effect 化已豁免的 app/ 副作用，缩小 tea-purity 豁免名单
**现状**：`check-tui-tea-purity.sh` EXEMPT 名单含 `slash.rs`、`slash/reflection.rs`、`slash/save.rs`、`slash/memory.rs`、`slash/suggestions.rs`、`slash/dialog.rs`、`util.rs`、`runtime.rs` 等 —— 这些文件内联了 `tokio::spawn`（reflection）、`block_in_place` 读剪贴板图片（/paste）、`copy_to_clipboard`、`agent_client.save/list_models`（含 block_on）等副作用，属**已知债务、显式豁免**。
**目标**：逐个把这些副作用描述为 `Effect`（`RunReflection`/`ApplyReflection`、`ReadClipboardImage`、`CopyToClipboard`、`SaveSession`、`ListModels`…）交由 `effect/executor` 执行，从 EXEMPT 名单移除对应文件。
**依赖**：相对独立，可与 S1–S4 并行。**收尾性质，建议最后或穿插。**

## 5. 建议顺序

```
S1（live tail，最小自包含，已有 build_task_window 可复用）
  └→ S2（output scroll/selection）   S3（status 去镜像）   ← 可并行
        └──────────┬──────────────────┘
                   S4（mouse_handler 选区统一，依赖 S2/S3）
S5（副作用 Effect 化，缩小豁免名单）  ← 全程可并行/穿插
```

## 6. 通用收口范式（每个子项都遵循）

1. 状态进对应 `*Model`，新增 `*Intent`/`*Change`，`apply()` 三路径单测。
2. 触发点改 intent；widget 直改方法收紧为内部/测试可见。
3. 渲染经 ViewModel（必要时启用现有闲置 assembler），删 widget 直读。
4. 新增对应 `.agents/hooks/check-tui-*.sh` guard 并接入架构守卫。
5. 全量 `cargo test -p cli` + clippy + 架构守卫通过。

## 7. 明确不做

- 不改 #56/#57/#58 已收口或进行中的范围。
- 不引入运行时 Theme（与 RenderCtx TODO 解耦，单独 feature）。
- 不重写渲染管线（属 #58）。
- 不追求"状态零留 widget"的严格诠释；维持单写入者/单向 + guard 范式。
