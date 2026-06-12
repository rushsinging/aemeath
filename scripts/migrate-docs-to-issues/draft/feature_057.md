<!-- Migrated from: docs/feature/archived/057-tui-toplevel-physical-cleanup.md -->
# Feature #57：TUI 目录物理收口 — 并入剩余 widget/service 目录、删 core shim

**状态**：✅ 已完成（2026-05-30 用户确认）

**优先级**：低

## 背景

feature #55 完成三层（render/adapter/app）落地与 core/state·core/update·model/session 清理后，对照架构 spec `2026-05-27-tui-model-view-architecture.md` 的目标目录，`apps/cli/src/tui/` 仍多出 6 个顶层目录。本 feature 专门收尾。

## 多出的 6 个顶层目录

1. `core/` — 仅含弃用说明的空 `mod.rs`。
2. `output_area/`（~1973 行）— 输出区 ratatui widget；spec 归属 `render/output/`。
3. `input/`（~1355 行）— 输入 widget；spec 归属 `render/input` + widget。
4. `display/`（~1282 行）— status_bar/theme/safe_text/task_window/syntax/dialog；spec 归属 `render/{status,dialog,theme}`。
5. `completion/`（~628 行）— 补全数据源。
6. `session/`（~408 行）— 会话生命周期。

## 完成情况

1. ✅ 删除 `core/` 空 shim。
2. ✅ `output_area/` 迁入 `render/output_area/`。
3. ✅ `input/` 迁入 `render/input/`，与 `model/input/` 物理分离。
4. ✅ `display/` 迁入 `render/display/`，status bar 相关 `#[path]` 测试引用同步调整。
5. ✅ `completion/` 拆入 `model/input/completion/`（纯解析、类型、命令/模型/session 候选组装与补全状态）与 `effect/completion/`（文件系统扫描候选生成），避免 Input Model 直接 IO。
6. ✅ `session/` 拆入 `effect/session/`（spawn/load/save/resume 副作用编排）；状态继续归 `model/runtime`。
7. ✅ 新增 `.agents/hooks/check-tui-toplevel-layout.sh`，白名单锁定 TUI 顶层目录为 spec 9 层（`app update model view_assembler view_model view_state render effect adapter`），禁止旧顶层模块路径回归；接入 `check-architecture-guards.sh`。

## 验证

`cargo test -p cli`、`.agents/hooks/check-architecture-guards.sh` 通过。2026-05-30 用户确认 feature #57 已完成。
