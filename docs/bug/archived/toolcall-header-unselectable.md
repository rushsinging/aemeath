# Tool call 标题行无法鼠标选中

## 症状
在 TUI 输出区域中，tool call 标题行（带 `●`/`✓`/`✗` 符号的行）无法通过鼠标点击选中内容。其他所有行（assistant 文本、tool result 等）均可正常选中。

## 根因
`mod.rs` 的 `render()` 方法中，三个 tool call 分支（`ToolCallRunning`、`ToolCallSuccess`、`ToolCallError`）在 169-212 行通过 `return` 提前退出，**跳过了后续 `screen_line_map` 的构建代码**（`compute_char_offsets` + `new_screen_map.push`）。

鼠标选中通过 `screen_line_map` 将屏幕坐标映射到逻辑行+字符偏移。tool call 行在 `screen_line_map` 中没有条目，`start_selection` 中的 `rel_row < self.screen_line_map.len()` 检查失败，选中操作静默失效。

## 修复
将 `compute_char_offsets` 和 `new_screen_map.push` 循环移到三个 tool call 提前 return 分支**之前**。所有行（包括 tool call）先注册屏幕映射，再进行各自的渲染分支。

后续二次修复：tool call 行的 `return` 分支使用特殊 Span（dot + text）跳过了选择高亮渲染。改为统一走 `render_line_with_selection`，dot 颜色通过 buf 后处理叠加。
**修复 commit**：538a619

## 回归测试
- tool call 行鼠标点击/拖拽选中
- tool call 行选中后复制的内容不应包含额外的不可见字符
- 非 tool call 行选中不受影响

## 关联
- 关联 Bug：#
- 涉及路径/模块：selection `←` screen_line_map `←` mod.rs render

---
**发现日期**：2026-04-25
**已归档**：2026-04-25，用户确认修复
