# Feature #63：TUI Block 抽象 trait 化 + 真正渲染树（嵌套规则）+ gutter

**状态**：✅ 已完成（2026-05-30 用户确认）

**优先级**：高

## 背景

#58 渲染管线统一后，输出区 block 仍以"自由函数 + match 分发"实现，缺真正的渲染树与嵌套规则；行首无 gutter 指示，块层次与状态在视觉上不易识别。详见 [spec](../superpowers/specs/2026-05-29-tui-block-trait-nesting-design.md) 与 [plan](../superpowers/plans/2026-05-29-tui-block-trait-nesting.md)。

## 解决方案

1. **BlockComponent trait** 取代自由函数+match 分发，`cache_version` 统一指纹。
2. **OutputViewModel 升 BlockNode 树**，`document_renderer` 递归 `render_tree` DFS 展平。
3. **嵌套规则** `nesting.rs`（`allowed_child` + `MAX_BLOCK_DEPTH=3`）+ assembler `push_child_checked` 构造期校验 + `check-tui-block-nesting.sh` guard。
4. **tool result 升为独立 `OutputBlockKind::ToolResult` 子块**：render 逻辑从 `tool_call` 移入 `blocks/tool_result.rs`，复用 `render_edit_diff`/`render_fenced_markdown`；结构隔离 #65（result 自有缓存块，fence 状态不跨块泄漏）。
5. **行首 gutter**（`render/output/gutter.rs`）：每行 [depth 缩进 + marker 列]，marker 静态按 kind/status（●/✓/✗、UserMessage `>`、其余空），仅首行画、后续等宽空白，组合期注入、不进 plain；选区列偏移补偿 `gutter_cols`。
6. 删除 `OutputViewModel.blocks` 旧路径 + `render_fenced_markdown` indent 参数。

## 视觉变化

- 所有 block 行首 2 列 gutter。
- tool result 作 depth-1 子块缩进 4 列。

## 验证

`feature/63-block-trait-nesting` 分支：472 测试通过 + clippy + 架构守卫通过。2026-05-30 用户确认 feature #63 已完成。

## 已知遗留

- `ToolCallBlockView.activity_summary` 为死字段（后续清理）。
- #65 仅加固不认领修复（由 #58 G2 + #63 共同结构隔离）。
- #76 属 #58 域不在 #63 范围。
