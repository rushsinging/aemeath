# Feature #55：TUI 架构收口 — render / adapter / app 三层落地 + 清理 legacy core

**状态**：✅ 已完成（2026-05-30 用户确认）

**优先级**：中

## 背景

feature #53（TUI Model/View 迁移）遗留收口。按 `docs/superpowers/specs/2026-05-27-tui-model-view-architecture.md` 的目标模块结构审计 `apps/cli/src/tui/`，新 Model/View 骨架已建立且依赖方向大体正确，但渲染散在 legacy `output_area/` + `display/` + `view_model/render.rs`，adapter 散在 `update/*_mapper.rs` + `core/*_adapter.rs`，`core/` 还塞着 `core/update/` `core/state/` `effect_runtime.rs` `slash` 等。

## 解决方案

1. 建立统一 `render/` 层，将 `output_area/`、`display/` 渲染逻辑、`view_model/render.rs` 收口进去。
2. 建立 `adapter/` 层，归拢 agent/terminal/task/hook event 适配。
3. 将 `core/` 收薄为 `app/`：原 `core/mod.rs`、`event`、`resize`、`run_loop`、`runtime`、`state`、`update`、`slash`、`util` 已迁入 `app/`；`core/` 仅保留弃用兼容 namespace。
4. 删除 legacy `core/update/`、`core/state/`。
5. `model/session/` 并入 `model/runtime/session_*`，去掉第 5 个 model context。
6. 补齐 `effect/executor.rs`、`view_state/cache.rs`。
7. 落地 spec §834 起的剩余架构 guard（render isolation / view-assembler boundary / adapter guard / output line legacy guard / effect boundary）。

## 后续

剩余 6 个顶层目录（`output_area/` `input/` `display/` `completion/` `session/` + `core/` shim）由 #57 物理收口。

## 验证

2026-05-30 用户确认 feature #55 已完成。
