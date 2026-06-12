<!-- Migrated from: docs/feature/archived/056-input-single-source-guard.md -->
# Feature #56：输入单一真相约束 — 禁止直接改 input_area 并加架构 guard

**状态**：✅ 已完成（2026-05-30 用户确认）

**优先级**：中

## 背景

2026-05-28 用户指出 `input_area` 所有直接修改（text 或 cursor）后未同步 `model.input.document`，突破了 #53 架构约束。"widget 与 model 漂移"正是 bug #79（Ctrl+A/E/Left/Right/End 输入位置错位、上下翻历史 + Ctrl+W delete_word 未同步）以及 #75（CJK 顺序）、#77（@ 补全回删）的同源根因；"改完手动 sync" 持续产 bug。

## 约束

输入的 text/cursor 业务真相只在 `model.input.document`（InputModel）。`InputArea` 是 View，必须由 model 单向派生（`adapter/input_widget.rs`）。update 层不得直接调 widget 可变方法再手动 sync。

## 解决方案（两层保障）

1. **静态 guard**：新增 `.agents/hooks/check-tui-input-single-source.sh`，禁止 adapter 之外对 `input_area` 调可变方法；仿 `check-tui-tea-purity.sh` 的 `EXEMPT_FILES` + `// allow input_single_source` 逃逸；接入 `check-architecture-guards.sh`（Stop hook）。
2. **类型级**：`InputArea` 可变方法可见性收紧为 `pub(in crate::tui::...)`，只对 adapter 模块可见，越界即编译失败。

## 完成内容

- 键盘输入、AskUserQuestion 自由输入、粘贴、补全确认、图片 pending count 更新均改为 `InputIntent → InputModel::apply → adapter/input_widget.rs`。
- `InputModel` 补齐 `ReplaceText`/`InsertNewline`/`DeleteWordBeforeCursor`/`AcceptCompletionValue`/`SetAttachmentCount` 等 intent；`InputDocument` 补齐 replace/delete-word/is-empty 等纯逻辑。
- `app/update`、`app/util`、`app/slash/suggestions` 改读 `model.input.document` 作为 text/cursor 业务真相。
- 新增 `check-tui-input-single-source.sh`，禁止 adapter/input_area 外直接改 `input_area`，禁止 model 外直接改 `model.input.document`，禁止 app/update 读 `input_area` text/cursor 作为业务真相。

## 验证

`cargo check -p cli`、`cargo test -p cli`、`.agents/hooks/check-architecture-guards.sh` 通过。2026-05-30 用户确认 feature #56 已完成。

## 关联

bug #79、#75、#77；feature #53、#55；spec `docs/superpowers/specs/2026-05-27-tui-model-view-architecture.md`。
