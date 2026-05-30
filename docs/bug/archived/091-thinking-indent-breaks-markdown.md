# Bug #91：TUI thinking block 渲染时被缩进，破坏 markdown 表头与代码块

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 渲染 / Thinking block 缩进 |

## 症状

LLM thinking 内容在 TUI 中渲染时整段被缩进（如开头加空格/制表符），破坏 markdown 表头与代码块对齐，表格首列与代码 fence 看起来错位。

## 根因

`OutputViewAssembler` 给 `Thinking` block 走 `prefix_lines("│ ", ..)`/类似前缀路径，对 markdown 表头/代码块这种依赖行首字符判定的语法不友好。

## 修复

- thinking block 不再加左侧 `│ ` 前缀，与 assistant text 一样顶格渲染，markdown 表格/代码块判定恢复。
- 仅在视觉上以 dim 颜色 + `💭` 图标行区分 thinking 与 assistant text。
- 同次 commit 顺带修 #63/#64/#90 中 result 渲染相关问题（diff 去重复缩进、result 渲染类型工具声明）。

## 相关提交

- `68c8325` fix(tui): thinking 顶格 + diff 去重复缩进 + result 渲染类型工具声明 (refs #91 #63 #64 #90)

## 验证

2026-05-30 用户确认 bug #91 已修复。
