# Feature #50：CLI 目录整理 — 收拢碎片、统一模块层级

**状态**：✅ 已完成（2026-05-30 用户确认）

**优先级**：中

## 背景

CLI 包内目录层级混乱：同名文件 + 目录并存（`tui/input_area.rs` 与 `tui/input_area/`、`tui/key_hints.rs` 与 `tui/key_hints/`）、render 三层散落（`src/render/`、`src/tui/render/`、`src/tui/widgets/` 职责重叠）、`status_bar.rs` 等 4 个分裂文件全摊在 `tui/` 根下、`application/` 与 `repl/` 废弃骨架仍占位。

## 解决方案

- 同名文件+目录统一为目录形式（`input_area.rs` → `input_area/mod.rs` 等）。
- 顶层 `src/render/` 内容合并到相应 widget 模块，删除冗余顶层 `src/render/`。
- 删除 `application/` 废弃骨架与已无引用的 repl 残留。
- 同时与 #47（DDD 设计）并轨：为 UI Domain 4 个 Context（Model/View/Update/Effect）提供物理基础。

## 关联

并入 #47 一并完成（已确认）。物理目录结构后续被 #55/#57 继续收口至 spec 9 层目标结构。

## 验证

2026-05-30 用户确认 feature #50 已完成。
