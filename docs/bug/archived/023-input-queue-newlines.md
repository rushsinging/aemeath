# Bug #23 Input queue 内容在 TUI 显示时未适配换行符

**状态**：✅ 已修复，用户已确认
**发现日期**：2026-04
**确认日期**：2026-04-29
**优先级**：中
**修复 commit**：待提交

## 症状

processing 状态下输入进入 input queue 后，TUI 上方展示 queued message 时没有正确处理消息里的换行符。

多行内容可能显示成一行、截断、缩进错乱，或后续行缺少队列前缀/样式，导致排队内容难以阅读。

## 预期行为

1. queued message 中的 `\n` 应按实际多行展示。
2. 第一行保留队列标识/序号，后续行使用一致缩进，明确属于同一条 queued message。
3. 长行仍应按 TUI 宽度正常 wrap，不应破坏 output area / spinner / input area 布局。
4. 空行应保留或以合理占位显示，避免多段内容被挤在一起。

## 根因

queued message 渲染路径直接把整条字符串作为一个 `Line` / `Span` 输出，没有先按换行拆分，也没有为后续行补齐缩进和样式。

此前 Input Queue 优化主要处理了队列位置与交互语义，未覆盖多行文本展示。

## 修复

- queued message 渲染前先按 `\n` 拆成多行。
- 第一行使用 `> ` 前缀。
- 后续行使用两个空格缩进。
- 保留空行。
- reserved height 改为按拆分后的实际展示行数计算，避免多行 queue 覆盖 spinner / input area。

## 涉及文件

- `aemeath-cli/src/tui/output_area/mod.rs`

## 验证

用户通过输入多行内容测试，已确认修复。
