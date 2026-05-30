# #74 TUI 执行 /reflect 后续文本颜色全部变暗（System 色泄漏）

## 症状
在 TUI 中执行 `/reflect` 后，reflection 输出及其**后续的普通/assistant 文本**全部呈现暗灰蓝色（System 样式），而非正常的 assistant 前景色。

## 根因
旧架构中 `ReflectionDone` 通过 `output_area.push_system(&output.content)` 以 `LineStyle::System` 推送整段 reflection 输出，旧渲染管线中 block 间共享可变样式状态，导致 System(Muted) 色泄漏到后续 block。

## 修复
#58 渲染管线重构结构性修复：
- 每个输出 block 独立从自身 kind/style 派生颜色，不存在跨 block 共享可变样式状态
- `push_system` 旧 API 已移除，所有输出通过 `ConversationModel` → `append_system_notice` 走新管线
- `blocks/mod.rs::test_render_block_assistant_after_system_does_not_inherit_dark` 回归测试确认隔离

## 涉及路径
- `apps/cli/src/tui/render/output/blocks/diagnostic.rs`（System block 渲染）
- `apps/cli/src/tui/render/output/blocks/mod.rs`（回归测试）
- `apps/cli/src/tui/app/update/ui_event.rs`（ReflectionDone 处理）
- `apps/cli/src/tui/app/update/notice.rs`（append_system_notice）

## 状态
已修复，用户已确认。
